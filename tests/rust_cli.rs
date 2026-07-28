use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    thread,
};

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use tempfile::tempdir;

fn serve_connection(mut stream: TcpStream) {
    let mut request = [0_u8; 4096];
    let bytes_read = stream.read(&mut request).expect("read mock request");
    let request = String::from_utf8_lossy(&request[..bytes_read]);
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .expect("request path");

    let body = match path {
        "/dianzan/1-1" => {
            r#"
            <html><body>
              <div class="Nbbs-tiezi-lists">
                <a href="/a/post1" title="周末观点">周末观点</a>
                <div class="left middle-list-user cblue cursor overhide">大V甲</div>
                <div class="left middle-list-post">07-27 09:30</div>
              </div>
            </body></html>
            "#
        }
        "/a/post1" => {
            r#"
            <html><head><title>周末观点</title></head><body>
              <div class="article-text p_coten">
                <p>这是用于端到端测试的淘股吧文章正文。</p>
                <p>它足够长，可以确认正文解析、哈希与持久化流程全部成功。</p>
                <script>window.bad = true;</script>
              </div>
            </body></html>
            "#
        }
        other => panic!("unexpected mock path: {other}"),
    };
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream
        .write_all(response.as_bytes())
        .expect("write mock response");
}

fn spawn_mock_server(expected_requests: usize) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
    let address = listener.local_addr().expect("mock address");
    let handle = thread::spawn(move || {
        for _ in 0..expected_requests {
            let (stream, _) = listener.accept().expect("accept mock request");
            serve_connection(stream);
        }
    });
    (format!("http://{address}"), handle)
}

#[test]
fn hot_command_persists_body_and_exports_the_same_run() {
    let temp = tempdir().expect("temp directory");
    let database = temp.path().join("tgb.db");
    let (base_url, server) = spawn_mock_server(2);

    let mut crawl = cargo_bin_cmd!("tgb");
    crawl
        .arg("--database")
        .arg(&database)
        .arg("--base-url")
        .arg(&base_url)
        .args(["--delay-ms", "0", "hot"])
        .args(["--from", "2026-07-27 00:00"])
        .args(["--to", "2026-07-27 23:59"])
        .args(["--pages", "1", "--fetch-body", "--concurrency", "1"]);
    crawl
        .assert()
        .success()
        .stdout(predicate::str::contains("\"run_id\": 1"))
        .stdout(predicate::str::contains("\"discovered\": 1"))
        .stdout(predicate::str::contains("\"parsed\": 1"))
        .stdout(predicate::str::contains("\"failed\": 0"));
    server.join().expect("mock server thread");

    let mut export = cargo_bin_cmd!("tgb");
    export
        .arg("--database")
        .arg(&database)
        .args(["export", "--run", "1", "--only-success"]);
    export
        .assert()
        .success()
        .stdout(predicate::str::contains("\"article_id\":\"post1\""))
        .stdout(predicate::str::contains("用于端到端测试"))
        .stdout(predicate::str::contains("window.bad").not());

    let mut show = cargo_bin_cmd!("tgb");
    show.arg("--database")
        .arg(&database)
        .args(["run", "show", "1"]);
    show.assert()
        .success()
        .stdout(predicate::str::contains("\"status\": \"success\""))
        .stdout(predicate::str::contains("\"parsed_count\": 1"));
}
