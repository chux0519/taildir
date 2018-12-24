use std::rc::Rc;
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

fn test_filter(name: &str) -> bool {
    name.ends_with(".log")
}

fn main() {
    let option = taildir::WatchOption::new(String::from("./test"), 5)
        .file_filter(Rc::new(test_filter));
    taildir::watch_dir(&option, &callback).unwrap_or(());
}
