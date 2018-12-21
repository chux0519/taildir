use taildir;
fn callback(name: &str, logs: Vec<String>) {
    if !logs.is_empty() {
        println!("Recieved {} error logs, file: {}", logs.len(), name);

        for log in logs {
            println!("{}", log);
        }
        // You may want to send to message queue, etc
    }
}

fn main() {
    let option = taildir::WatchOption::new(String::from("./"), 5);
    taildir::watch_dir(&option, &callback).unwrap_or(());
}
