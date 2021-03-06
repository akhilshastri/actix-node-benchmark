use structopt::StructOpt;
use tokio::process::Command;
use std::process::Stdio;
use tokio::time::{delay_for, Duration};
use psutil::process::processes;

/// Automation for running load tests and gathering stats
/// It uses wrk under the hood, make sure to have it in the PATH
#[derive(StructOpt)]
struct Opt {
    /// IP address of target hast
    #[structopt(short="h", long="host", default_value="127.0.0.1")]
    host: std::net::IpAddr,
    /// Concurrency limit
    #[structopt(short="c", long="mc", default_value="128")]
    max_concurrency: u16,
    /// Node port
    #[structopt(short="n", long="np", default_value="3000")]
    node_port: u16,
    /// Actix port
    #[structopt(short="a", long="ap", default_value="3002")]
    actix_port: u16,
    /// Monitor local processes: node, actix and postgres
    #[structopt(short="m", long="monitor")]
    monitor: bool,
    /// Measurement time in seconds
    #[structopt(short="t", long="time", default_value="60")]
    time: u16,
}

fn wrk(concurrency: u16, url: &String, delay: u16) -> Command {
    let mut wrk = Command::new("wrk");
    wrk .arg(format!("-t {}", concurrency/10+1))
        .arg(format!("-c {}", concurrency))
        .arg(format!("-d{}s", delay))
        .arg(url)
        .kill_on_drop(true)
        .stdout(Stdio::piped());
    wrk
}

#[derive(Default)]
struct ProcessesReport {
    postgres: ProcessReport,
    node: ProcessReport,
    actix: ProcessReport,
}

#[derive(Default)]
struct ProcessReport {
    cpu: f32,
    mem: u64,
}

async fn monitor_processes() -> anyhow::Result<ProcessesReport> {
    let procs: Vec<_> = processes()?
        .into_iter()
        .filter_map(|p| p.ok())
        .collect();

    delay_for(Duration::from_secs(5)).await;

    let proc_stats = |name| procs.iter()
            .filter(|p| p.name().is_ok() && p.name().unwrap().contains(name))
            .cloned()
            .fold(
                ProcessReport::default(), 
                |mut acc, mut p| {
                    acc.cpu += p.cpu_percent().unwrap();
                    acc.mem += p.memory_info().unwrap().rss();
                    acc
                }
            );

    Ok(ProcessesReport {
        postgres: proc_stats("postgres"),
        node: proc_stats("node"),
        actix: proc_stats("actix"),
    })
}

#[derive(Default)]
struct WrkStats {
    latency: f32,
    rps: usize,
}

fn process_wrk(out: Vec<u8>) -> anyhow::Result<WrkStats> {
    let stdout = String::from_utf8(out)?;
    let latency_re = regex::Regex::new(r"Latency\s+(\d+\.\d+)(\w+)")?;
    let rps_re = regex::Regex::new(r"Requests/sec:\s+(\d+)")?;
    let mut res = WrkStats::default();

    for line in stdout.lines() {
        if let Some(latency) = latency_re.captures(line) {
            res.latency = latency.get(1).unwrap().as_str().parse()?;
            res.latency *= match latency.get(2).unwrap().as_str() {
                "s" => 1000f32,
                "us" => 0.001f32,
                _ => 1f32,
            }
        }
        if let Some(rps) = rps_re.captures(line) {
            res.rps = rps.get(1).unwrap().as_str().parse()?;
        }
    };
    Ok(res)
}

struct Results {
    test: String,
    name: String,
    concurrency: u16,
    proc_stats: ProcessesReport,
    wrk_stats: WrkStats,
}

use itertools::Itertools;
use std::cmp::max;

fn bars(n: usize) -> String {
    std::iter::repeat("*").take(n).collect::<String>()
}

fn print_charts(data: &Vec<Results>, width: usize) {
    let (max_lat, max_rps) = data.iter().fold((0f32, 0), |acc, res| 
        ( acc.0.max(res.wrk_stats.latency),
        max(acc.1, res.wrk_stats.rps) ));

    println!("\nLatency in ms (lower is better)");
    for ((_test, conc), results) in &data.into_iter().group_by(|r| (&r.test, r.concurrency)) {
        println!("\nconcurrent load {}", conc);
        let results: Vec<_> = results.collect();
        for result in &results {
            let size = (result.wrk_stats.latency * width as f32 / max_lat) as usize + 1;
            println!("{:6} |{:width$}|", result.name, bars(size), width = width+2);
        }
    }

    println!("\nRequests per second (higher is better)");
    for ((_test, conc), results) in &data.into_iter().group_by(|r| (&r.test, r.concurrency)) {
        println!("\nconcurrent load {}", conc);
        let results: Vec<_> = results.collect();
        for result in &results {
            let size = (result.wrk_stats.rps * width / max_rps) as usize + 1;
            println!("{:6} |{:width$}|", result.name, bars(size), width = width+2);
        }
    }  
}

#[tokio::main(core_threads = 1)]
async fn main() -> anyhow::Result<()> {
    let opt = Opt::from_args();
    let node_url = format!("http://{}:{}/tasks", opt.host, opt.node_port);
    let actix_url = format!("http://{}:{}/tasks", opt.host, opt.actix_port);
    let mut results: Vec<Results> = Vec::new();

    // table header
    println!("Target,\tConcur,\tPG cpu,\tmem,\tND cpu,\tmem,\tAX cpu,\tmem,\tlat ms,\trps");

    for test in &["", "?summary=wherever&full=true&limit=10"] {
        println!("Starting test /tasks{}", test);
        let url = |base: &String| format!("{}{}", base, test);
        let mut c = 1u16;

        while c < opt.max_concurrency {
            println!("concurrent load = {}", c);
            for sol in &[("node", url(&node_url)), ("actix", url(&actix_url))] {
                let wrk = wrk(c, &sol.1, opt.time).output();

                let proc_stats = if opt.monitor {
                        delay_for(Duration::from_secs(opt.time as u64 / 2)).await;
                        monitor_processes().await?
                    } else {
                        ProcessesReport::default()
                    };

                let wrk_stats = process_wrk(wrk.await?.stdout)?;

                println!("{:5},\t{},\t{:.2},\t{:3},\t{:.2},\t{:3},\t{:.2},\t{:3},\t{:.2},\t{}", 
                    sol.0, c, 
                    proc_stats.postgres.cpu / 100f32,
                    proc_stats.postgres.mem / 1024 / 1024,
                    proc_stats.node.cpu / 100f32,
                    proc_stats.node.mem / 1024 / 1024,
                    proc_stats.actix.cpu / 100f32,
                    proc_stats.actix.mem / 1024 / 1024,
                    wrk_stats.latency,
                    wrk_stats.rps
                );

                results.push(
                    Results { 
                        test: test.to_string(), name: sol.0.to_string(), concurrency: c, proc_stats, wrk_stats 
                    }
                );
            }
            c *= 2;
        }

        print_charts(&results, 100);
        results.truncate(0);
    }
    Ok(())

}
