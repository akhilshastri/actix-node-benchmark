use std::io;

use futures::FutureExt;

use super::{PgConnection, Task};
use actix::{Handler, Message, ResponseFuture};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct GetTasks {
    summary: Option<String>,
    assignee_name: Option<String>,
}

impl Message for GetTasks {
    type Result = io::Result<Vec<Task>>;
}

impl Handler<GetTasks> for PgConnection {
    type Result = ResponseFuture<Result<Vec<Task>, io::Error>>;

    fn handle(
        &mut self, GetTasks { summary, assignee_name }: GetTasks, _: &mut Self::Context,
    ) -> Self::Result {
		let cl = self.client();
		let like = |s| format!("%{}%", s);
        /*let st = if summary.is_some() && assignee_name.is_some() {
            cl.tasks_name_summary
        } else if summary.is_some() {
            cl.tasks_summary
        } else if assignee_name.is_some() {
            cl.tasks_name
        } else {
            cl.tasks
        };*/
        let query = async move {
            if summary.is_some() && assignee_name.is_some() {
                let summary = like(summary.unwrap());
                let assignee_name = like(assignee_name.unwrap());
                cl.conn.query(&cl.tasks_name_summary, &[&summary, &assignee_name]).await
            } else if summary.is_some() {
                let summary = like(summary.unwrap());
                cl.conn.query(&cl.tasks_summary, &[&summary]).await
            } else if assignee_name.is_some() {
                let assignee_name = like(assignee_name.unwrap());
                cl.conn.query(&cl.tasks_name, &[&assignee_name]).await
            } else {
                cl.conn.query(&cl.tasks, &[]).await
            }
        };

        let get_tasks = query.map(|res| match res {
            Err(e) => Err(io::Error::new(io::ErrorKind::Other, format!("{:?}", e))),
            Ok(rows) => Ok(rows
                .iter()
                .map(|row| Task {
                    id: row.get(0),
                    summary: row.get(1),
                    description: row.get(2),
                    assignee_id: row.get(3),
                    assignee_name: row.get(4),
                })
                .collect()),
        });
        Box::pin(get_tasks)
    }
}
