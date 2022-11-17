use std::io::BufRead;
use std::io::BufReader;
use std::io::Read;
// use std::io::Write;

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use clap::{AppSettings, Parser, Subcommand};

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
#[clap(global_setting(AppSettings::DisableHelpSubcommand))]
struct Args {
    #[clap(value_parser)]
    path: String,

    #[clap(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Put {
        #[clap(long, value_parser)]
        topic: Option<String>,
        #[clap(long, value_parser)]
        attribute: Option<String>,
    },

    Cat {
        #[clap(short, long, action)]
        follow: bool,
        #[clap(long, action)]
        sse: bool,
        #[clap(short, long, value_parser)]
        last_id: Option<i64>,
    },

    Call {
        #[clap(long, value_parser)]
        topic: Option<String>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct Frame {
    #[serde(skip_serializing_if = "Option::is_none")]
    topic: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    attribute: Option<String>,
    data: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Row {
    id: i64,
    frame: Frame,
    stamp: String,
}

fn main() {
    let args = Args::parse();

    let conn = sqlite::open(&args.path).unwrap();
    store_create(&conn);

    match &args.command {
        Commands::Put { topic, attribute } => {
            /*
            if *follow {
                for line in std::io::stdin().lock().lines() {
                    put_one(&conn, None, None, None, &None, &None, line.unwrap());
                }
                return;
            }
            */

            /*
            if let Some(sse) = sse {
                if *last_id {
                    let mut q = conn
                        .prepare(
                            "
                            SELECT source_id
                            FROM stream
                            WHERE source = ?
                            ORDER BY id DESC
                            LIMIT 1",
                        )
                        .unwrap()
                        .bind(1, sse.as_bytes())
                        .unwrap();
                    if let sqlite::State::Done = q.next().unwrap() {
                        println!("0");
                        return;
                    }
                    let id = q.read::<i64>(0).unwrap();
                    println!("{}", id);
                    return;
                }

                let mut stdin = BufReader::new(std::io::stdin());
                while let Some(event) = parse_sse(&mut stdin) {
                    put_one(
                        &conn,
                        event.id,
                        Some(sse.to_string()),
                        None,
                        &None,
                        &None,
                        event.data,
                    );
                }
                return;
            }
            */

            let mut data = String::new();
            std::io::stdin().read_to_string(&mut data).unwrap();
            println!(
                "{}",
                store_put(&conn, data, topic.clone(), attribute.clone())
            );
        }

        Commands::Cat {
            follow,
            sse,
            last_id,
        } => {
            // send a comment to establish the connection
            if *sse {
                println!(": welcome");
            }

            let mut last_id = last_id.unwrap_or(0);

            loop {
                let rows = store_cat(&conn, last_id);
                for row in &rows {
                    let data = serde_json::to_string(&row).unwrap();
                    match sse {
                        true => {
                            println!("id: {}", row.id);
                            // let data = row.data.trim().replace("\n", "\ndata: ");
                            println!("data: {}\n", data);
                        }

                        false => println!("{}", data),
                    }
                    last_id = row.id;
                }
                if !follow {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }

        Commands::Call { topic } => {
            let mut data = String::new();
            std::io::stdin().read_to_string(&mut data).unwrap();
            let _id = store_put(&conn, data, topic.clone(), Some(".request".into()));
        }
    }
}

#[derive(Debug, PartialEq)]
struct Event {
    data: String,
    event: Option<String>,
    id: Option<i64>,
}

fn parse_sse<R: Read>(buf: &mut BufReader<R>) -> Option<Event> {
    let mut line = String::new();

    let mut data = Vec::<String>::new();
    let mut id: Option<i64> = None;

    loop {
        line.clear();
        let n = buf.read_line(&mut line).unwrap();
        if n == 0 {
            // stream interrupted
            return None;
        }

        if line == "\n" {
            // end of event, emit
            break;
        }

        let (field, rest) = line.split_at(line.find(":").unwrap() + 1);
        let rest = rest.trim();
        match field {
            // comment
            ":" => (),
            "id:" => id = Some(rest.parse::<i64>().unwrap()),
            "data:" => data.push(rest.to_string()),
            _ => todo!(),
        };
    }

    return Some(Event {
        data: data.join(" "),
        event: None,
        id: id,
    });
}

fn store_create(conn: &sqlite::Connection) {
    conn.execute(
        "
        CREATE TABLE IF NOT EXISTS stream (
        id INTEGER PRIMARY KEY,
        frame TEXT NOT NULL,
        stamp TEXT NOT NULL
        )",
    )
    .unwrap();
}

fn store_put(
    conn: &sqlite::Connection,
    data: String,
    topic: Option<String>,
    attribute: Option<String>,
) -> i64 {
    let frame = Frame {
        topic: topic.clone(),
        attribute: attribute.clone(),
        data: data.trim().to_string(),
    };
    let frame = serde_json::to_string(&frame).unwrap();

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
        .to_string();

    let mut q = conn
        .prepare(
            "INSERT INTO stream (frame, stamp)
             VALUES (?, ?)
             RETURNING id;",
        )
        .unwrap()
        .bind(1, &*frame)
        .unwrap()
        .bind(2, &*stamp)
        .unwrap();

    assert_eq!(q.next().unwrap(), sqlite::State::Row);
    let id = q.read::<i64>(0).unwrap();
    assert_eq!(q.next().unwrap(), sqlite::State::Done);
    return id;
}

fn store_cat(conn: &sqlite::Connection, last_id: i64) -> Vec<Row> {
    // cat should return an iterator
    let mut ret = Vec::<Row>::new();
    let mut cur = conn
        .prepare(
            "SELECT
                    id, frame, stamp
                FROM stream
                WHERE id > ?
                ORDER BY id ASC",
        )
        .unwrap()
        .bind(1, last_id)
        .unwrap()
        .into_cursor();

    while let Some(Ok(r)) = cur.next() {
        let frame = r.get::<String, _>(1);
        let frame: Frame = serde_json::from_str(&frame).unwrap();
        let row = Row {
            id: r.get::<i64, _>(0),
            frame: frame,
            stamp: r.get::<String, _>(2),
        };
        ret.push(row);
    }

    return ret;
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;
    // use pretty_assertions::assert_eq;

    #[test]
    fn test_store() {
        let conn = sqlite::open(":memory:").unwrap();
        store_create(&conn);
        let id = store_put(&conn, "foo".into(), None, None);
        assert_eq!(id, 1);
        let rows = store_cat(&conn, 0);
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn test_parse_sse() {
        let mut buf = BufReader::new(
            indoc! {"
        : welcome
        id: 1
        data: foo
        data: bar

        id: 2
        data: hai

        "}
            .as_bytes(),
        );

        let event = parse_sse(&mut buf).unwrap();
        assert_eq!(
            event,
            Event {
                data: "foo bar".into(),
                event: None,
                id: Some(1),
            }
        );

        let event = parse_sse(&mut buf).unwrap();
        assert_eq!(
            event,
            Event {
                data: "hai".into(),
                event: None,
                id: Some(2),
            }
        );
    }
}
