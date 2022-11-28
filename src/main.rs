use std::io::BufRead;
use std::io::BufReader;
use std::io::Read;
use std::io::Write;

use std::str::FromStr;

use serde::{Deserialize, Serialize};

use lmdb::Cursor;
use lmdb::Transaction;

use clap::{AppSettings, Parser, Subcommand};

// POLL_INTERVAL is the number of milliseconds to wait between polls when watching for
// additions to the stream
// todo: investigate switching to: https://docs.rs/notify/latest/notify/
const POLL_INTERVAL: u64 = 5;

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

    Get {
        #[clap(long, value_parser)]
        id: String,
    },

    Cat {
        #[clap(short, long, action)]
        follow: bool,
        #[clap(long, action)]
        sse: bool,
        #[clap(short, long, value_parser)]
        last_id: Option<scru128::Scru128Id>,
    },

    Call {
        #[clap(long, value_parser)]
        topic: String,
    },

    Serve {
        #[clap(long, value_parser)]
        topic: String,
        #[clap(value_parser)]
        command: String,
        #[clap(value_parser)]
        args: Vec<String>,
    },
}

#[derive(PartialEq, Debug, Serialize, Deserialize)]
struct Frame {
    id: scru128::Scru128Id,
    #[serde(skip_serializing_if = "Option::is_none")]
    topic: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    attribute: Option<String>,
    data: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ResponseFrame {
    source_id: scru128::Scru128Id,
    data: String,
}

fn main() {
    let params = Args::parse();
    let env = store_open(std::path::Path::new(&params.path));

    match &params.command {
        Commands::Put { topic, attribute } => {
            let mut data = String::new();
            std::io::stdin().read_to_string(&mut data).unwrap();
            println!(
                "{}",
                store_put(&env, topic.clone(), attribute.clone(), data)
            );
        }

        Commands::Get { id } => {
            let id = scru128::Scru128Id::from_str(id).unwrap();
            let frame = store_get(&env, id);
            let frame = serde_json::to_string(&frame).unwrap();
            println!("{}", frame);
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

            let env = std::sync::Arc::new(env);
            let frames = store_cat(env.clone(), *last_id, *follow);
            for frame in frames {
                let data = serde_json::to_string(&frame).unwrap();
                match sse {
                    true => {
                        println!("id: {}", frame.id);
                        println!("data: {}\n", data);
                    }

                    false => println!("{}", data),
                }
            }
        }

        Commands::Call { topic } => {
            let mut data = String::new();
            std::io::stdin().read_to_string(&mut data).unwrap();

            let env = std::sync::Arc::new(env);
            let id = store_put(&env, Some(topic.clone()), Some(".request".into()), data);
            let frames = store_cat(env.clone(), Some(id), true);
            let frames = frames.iter().filter(|frame| {
                frame.topic == Some(topic.to_string())
                    && frame.attribute == Some(".response".into())
            });
            for frame in frames {
                let response: ResponseFrame = serde_json::from_str(&frame.data).unwrap();
                if response.source_id == id {
                    print!("{}", response.data);
                }
                break;
            }
        }

        Commands::Serve {
            topic,
            command,
            args,
        } => {
            let env = std::sync::Arc::new(env);

            let last_id = store_cat(env.clone(), None, false)
                .iter()
                .filter(|frame| {
                    frame.topic == Some(topic.to_string())
                        && frame.attribute == Some(".response".into())
                })
                .last()
                .and_then(|frame| {
                    Some(
                        serde_json::from_str::<ResponseFrame>(&frame.data)
                            .unwrap()
                            .source_id,
                    )
                });

            let frames = store_cat(env.clone(), last_id, true);
            let frames = frames.iter().filter(|frame| {
                frame.topic == Some(topic.to_string()) && frame.attribute == Some(".request".into())
            });

            for frame in frames {
                let mut p = std::process::Command::new(command)
                    .args(args)
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .spawn()
                    .expect("failed to spawn");
                {
                    let mut stdin = p.stdin.take().expect("failed to take stdin");
                    stdin.write_all(frame.data.as_bytes()).unwrap();
                }
                let res = p.wait_with_output().unwrap();
                let data = String::from_utf8(res.stdout).unwrap();
                let res = ResponseFrame {
                    source_id: frame.id,
                    data: data,
                };
                let data = serde_json::to_string(&res).unwrap();
                let _ = store_put(&env, Some(topic.clone()), Some(".response".into()), data);
            }
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

fn store_open(path: &std::path::Path) -> lmdb::Environment {
    std::fs::create_dir_all(path).unwrap();
    let env = lmdb::Environment::new()
        .set_map_size(10 * 10485760)
        .open(path)
        .unwrap();
    return env;
}

fn store_put(
    env: &lmdb::Environment,
    topic: Option<String>,
    attribute: Option<String>,
    data: String,
) -> scru128::Scru128Id {
    let id = scru128::new();

    let frame = Frame {
        id: id,
        topic: topic.clone(),
        attribute: attribute.clone(),
        data: data.trim().to_string(),
    };
    let frame = serde_json::to_vec(&frame).unwrap();

    let db = env.open_db(None).unwrap();
    let mut txn = env.begin_rw_txn().unwrap();
    txn.put(
        db,
        // if I understand the docs right, this should be 'to_ne_bytes', but that doesn't
        // work
        &id.to_u128().to_be_bytes(),
        &frame,
        lmdb::WriteFlags::empty(),
    )
    .unwrap();
    txn.commit().unwrap();

    return id;
}

fn store_get(env: &lmdb::Environment, id: scru128::Scru128Id) -> Option<Frame> {
    let db = env.open_db(None).unwrap();
    let txn = env.begin_ro_txn().unwrap();
    match txn.get(db, &id.to_u128().to_be_bytes()) {
        Ok(value) => Some(serde_json::from_slice(&value).unwrap()),
        Err(lmdb::Error::NotFound) => None,
        Err(err) => panic!("store_get: {:?}", err),
    }
}

fn store_cat(
    env: std::sync::Arc<lmdb::Environment>,
    last_id: Option<scru128::Scru128Id>,
    follow: bool,
) -> std::sync::mpsc::Receiver<Frame> {
    let (tx, rx) = std::sync::mpsc::sync_channel::<Frame>(0);
    std::thread::spawn(move || {
        let mut last_id = last_id;
        let db = env.open_db(None).unwrap();
        loop {
            let txn = env.begin_ro_txn().unwrap();
            let mut c = txn.open_ro_cursor(db).unwrap();
            let it = match last_id {
                Some(key) => {
                    let mut i = c.iter_from(&key.to_u128().to_be_bytes());
                    i.next();
                    i
                }
                None => c.iter_start(),
            };

            for item in it {
                let (_, value) = item.unwrap();
                let frame: Frame = serde_json::from_slice(&value).unwrap();
                last_id = Some(frame.id);
                if tx.send(frame).is_err() {
                    return;
                }
            }
            if !follow {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(POLL_INTERVAL));
        }
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;
    use temp_dir::TempDir;
    // use pretty_assertions::assert_eq;

    #[test]
    fn test_store() {
        let d = TempDir::new().unwrap();
        let env = std::sync::Arc::new(store_open(&d.path()));

        let id = store_put(&env, None, None, "foo".into());
        assert_eq!(store_cat(env.clone(), None, false).iter().count(), 1);

        let frame = store_get(&env, id).unwrap();
        assert_eq!(
            frame,
            Frame {
                id: id,
                topic: None,
                attribute: None,
                data: "foo".into()
            }
        );

        // skip with last_id
        assert_eq!(store_cat(env.clone(), Some(id), false).iter().count(), 0);
    }

    #[test]
    fn test_store_cat_follow() {
        let d = TempDir::new().unwrap();
        let env = std::sync::Arc::new(store_open(&d.path()));

        let rx = store_cat(env.clone(), None, true);

        let id = store_put(&env, None, None, "foo".into());
        assert_eq!(rx.recv().unwrap().id, id);

        // no updates
        assert!(rx
            .recv_timeout(std::time::Duration::from_millis(20))
            .is_err());

        // an update
        let id = store_put(&env, None, None, "foo".into());
        assert_eq!(rx.recv().unwrap().id, id);

        // check sender cleanly stops
        drop(rx);

        let _ = store_put(&env, None, None, "foo".into());
        std::thread::sleep(std::time::Duration::from_millis(20));
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
