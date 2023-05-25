use std::io::BufRead;
use std::io::Read;
use std::io::Write;

use std::str::FromStr;

use clap::{Parser, Subcommand};

// POLL_INTERVAL is the number of milliseconds to wait between polls when watching for
// additions to the stream
// todo: investigate switching to: https://docs.rs/notify/latest/notify/
const POLL_INTERVAL: u64 = 5;

#[derive(Parser, Debug)]
#[clap(version)]
struct Args {
    #[clap(value_parser)]
    path: String,

    #[clap(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Insert a payload into the store from stdin, with optional follow
    Put {
        #[clap(long, value_parser)]
        topic: Option<String>,
        #[clap(long, value_parser)]
        attribute: Option<String>,
        #[clap(short, long, action)]
        follow: bool,
    },

    /// Fetch a specific payload by ID
    Get {
        #[clap(value_parser)]
        id: String,
    },

    /// Stream payloads from store with optional last_id and follow
    Cat {
        #[clap(short, long, action)]
        follow: bool,
        #[clap(long, action)]
        sse: bool,
        #[clap(short, long, value_parser)]
        last_id: Option<scru128::Scru128Id>,
    },

    /// Send request to topic, wait for response, print output
    Call {
        #[clap(long, value_parser)]
        topic: String,
    },

    /// Listen for requests, execute command, write output back
    Serve {
        #[clap(long, value_parser)]
        topic: String,
        #[clap(value_parser)]
        command: String,
        #[clap(value_parser)]
        args: Vec<String>,
    },
}

fn main() {
    let params = Args::parse();
    let env = xs_lib::store_open(std::path::Path::new(&params.path));

    match &params.command {
        Commands::Put {
            topic,
            attribute,
            follow,
        } => {
            if *follow {
                let stdin = std::io::stdin();
                for line in stdin.lock().lines() {
                    let data = line.unwrap();
                    println!(
                        "{}",
                        xs_lib::store_put(&env, topic.clone(), attribute.clone(), data)
                    );
                }
            } else {
                let mut data = String::new();
                std::io::stdin().read_to_string(&mut data).unwrap();
                println!(
                    "{}",
                    xs_lib::store_put(&env, topic.clone(), attribute.clone(), data)
                );
            }
        }

        Commands::Get { id } => {
            let id = scru128::Scru128Id::from_str(id).unwrap();
            let frame = xs_lib::store_get(&env, id);
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

            let mut last_id = *last_id;

            let mut signals =
                signal_hook::iterator::Signals::new(signal_hook::consts::TERM_SIGNALS).unwrap();

            loop {
                let frames = xs_lib::store_cat(&env, last_id);
                for frame in frames {
                    last_id = Some(frame.id);
                    let data = serde_json::to_string(&frame).unwrap();
                    match sse {
                        true => {
                            println!("id: {}", frame.id);
                            println!("data: {}\n", data);
                        }

                        false => println!("{}", data),
                    }
                }
                if !follow {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(POLL_INTERVAL));
                if signals.pending().next().is_some() {
                    return;
                }
            }
        }

        Commands::Call { topic } => {
            let mut data = String::new();
            std::io::stdin().read_to_string(&mut data).unwrap();

            let id = xs_lib::store_put(&env, Some(topic.clone()), Some(".request".into()), data);
            let mut last_id = Some(id);

            let mut signals =
                signal_hook::iterator::Signals::new(signal_hook::consts::TERM_SIGNALS).unwrap();

            loop {
                let frames = xs_lib::store_cat(&env, last_id);
                for frame in frames {
                    last_id = Some(frame.id);

                    if frame.topic != Some(topic.to_string())
                        || frame.attribute != Some(".response".into())
                    {
                        continue;
                    }

                    let response: xs_lib::ResponseFrame =
                        serde_json::from_str(&frame.data).unwrap();
                    if response.source_id == id {
                        print!("{}", response.data);
                    }
                    return;
                }

                std::thread::sleep(std::time::Duration::from_millis(POLL_INTERVAL));
                if signals.pending().next().is_some() {
                    return;
                }
            }
        }

        Commands::Serve {
            topic,
            command,
            args,
        } => {
            let mut last_id = xs_lib::store_cat(&env, None)
                .iter()
                .filter(|frame| {
                    frame.topic == Some(topic.to_string())
                        && frame.attribute == Some(".response".into())
                })
                .last()
                .map(|frame| {
                    serde_json::from_str::<xs_lib::ResponseFrame>(&frame.data)
                        .unwrap()
                        .source_id
                });

            let mut signals =
                signal_hook::iterator::Signals::new(signal_hook::consts::TERM_SIGNALS).unwrap();

            loop {
                let frames = xs_lib::store_cat(&env, last_id);
                for frame in frames {
                    last_id = Some(frame.id);

                    if frame.topic != Some(topic.to_string())
                        || frame.attribute != Some(".request".into())
                    {
                        continue;
                    }

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
                    let res = xs_lib::ResponseFrame {
                        source_id: frame.id,
                        data,
                    };
                    let data = serde_json::to_string(&res).unwrap();
                    let _ = xs_lib::store_put(
                        &env,
                        Some(topic.clone()),
                        Some(".response".into()),
                        data,
                    );
                }

                std::thread::sleep(std::time::Duration::from_millis(POLL_INTERVAL));
                if signals.pending().next().is_some() {
                    return;
                }
            }
        }
    }
}
