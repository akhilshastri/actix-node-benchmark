use serde::Serialize;
use tokio_pg_mapper::PostgresMapper;

#[derive(Serialize, PostgresMapper)]
#[pg_mapper(table = "task")]
pub struct Task {
    pub id: i32,
    pub summary: String,
    pub description: Option<String>,
    pub assignee_id: i32,
    pub assignee_name: String,
}
