//! External application API transport: a WebSocket server on 127.0.0.1
//! only (never another interface), off unless enabled in the settings.
//! Connection threads pass text frames to the UI thread, which answers
//! them through [`crate::external_api::handle`].
//! spec: 70-advanced/external-api.md §1

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::external_api::{self, Host, Session};

enum Incoming {
    Open(u64, Sender<String>),
    Text(u64, String),
    Closed(u64),
}

struct Conn {
    tx: Sender<String>,
    session: Session,
}

pub struct Server {
    pub port: u16,
    rx: Receiver<Incoming>,
    conns: HashMap<u64, Conn>,
    delayed: Vec<(Instant, u64, String)>,
    stop: Arc<AtomicBool>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn serve(stream: TcpStream, id: u64, to_ui: Sender<Incoming>, stop: Arc<AtomicBool>) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let Ok(mut ws) = tungstenite::accept(stream) else {
        return;
    };
    let _ = ws
        .get_ref()
        .set_read_timeout(Some(Duration::from_millis(20)));
    let (tx, from_ui) = channel::<String>();
    if to_ui.send(Incoming::Open(id, tx)).is_err() {
        return;
    }
    while !stop.load(Ordering::Relaxed) {
        match ws.read() {
            Ok(tungstenite::Message::Text(t)) => {
                if to_ui.send(Incoming::Text(id, t.to_string())).is_err() {
                    break;
                }
            }
            Ok(tungstenite::Message::Close(_)) => break,
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => break,
        }
        while let Ok(out) = from_ui.try_recv() {
            if ws.send(tungstenite::Message::text(out)).is_err() {
                let _ = to_ui.send(Incoming::Closed(id));
                return;
            }
        }
    }
    let _ = ws.close(None);
    let _ = to_ui.send(Incoming::Closed(id));
}

impl Server {
    /// Listens on 127.0.0.1:`port`.
    pub fn start(port: u16) -> std::io::Result<Self> {
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))?;
        listener.set_nonblocking(true)?;
        let (tx, rx) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let st = stop.clone();
        std::thread::Builder::new()
            .name("keyform-external-api".into())
            .spawn(move || {
                let mut next = 1u64;
                while !st.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((s, addr)) => {
                            // Only local peers (the socket is bound to 127.0.0.1).
                            if !addr.ip().is_loopback() {
                                continue;
                            }
                            let (tx, st2, id) = (tx.clone(), st.clone(), next);
                            next += 1;
                            let _ = std::thread::Builder::new()
                                .name(format!("keyform-external-api-{id}"))
                                .spawn(move || serve(s, id, tx, st2));
                        }
                        Err(_) => std::thread::sleep(Duration::from_millis(50)),
                    }
                }
            })?;
        Ok(Self {
            port,
            rx,
            conns: HashMap::new(),
            delayed: Vec::new(),
            stop,
        })
    }

    pub fn connections(&self) -> usize {
        self.conns.len()
    }

    /// Answers waiting requests; sends delayed replies that are due.
    pub fn poll(&mut self, host: &mut dyn Host, now_ms: u64) {
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Incoming::Open(id, tx) => {
                    self.conns.insert(
                        id,
                        Conn {
                            tx,
                            session: Session::default(),
                        },
                    );
                }
                Incoming::Closed(id) => {
                    self.conns.remove(&id);
                }
                Incoming::Text(id, text) => {
                    let Some(c) = self.conns.get_mut(&id) else {
                        continue;
                    };
                    for o in external_api::handle(&text, &mut c.session, host, now_ms) {
                        if o.delay_ms == 0 {
                            let _ = c.tx.send(o.text);
                        } else {
                            self.delayed.push((
                                Instant::now() + Duration::from_millis(o.delay_ms),
                                id,
                                o.text,
                            ));
                        }
                    }
                }
            }
        }
        let now = Instant::now();
        let conns = &self.conns;
        self.delayed.retain(|(at, id, text)| {
            if *at > now {
                return true;
            }
            if let Some(c) = conns.get(id) {
                let _ = c.tx.send(text.clone());
            }
            false
        });
    }

    /// Sends an event to the subscribed connections (§4.1 "Events").
    pub fn event(&self, method: &str, data: &serde_json::Value, now_ms: u64) {
        for c in self.conns.values() {
            if let Some(t) = external_api::event(&c.session, method, data.clone(), now_ms) {
                let _ = c.tx.send(t);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::external_api::{ApiError, DocInfo, ParamInfo};

    struct Minimal;

    impl Host for Minimal {
        fn documents(&self) -> Vec<DocInfo> {
            Vec::new()
        }
        fn current_document(&self) -> Option<String> {
            None
        }
        fn current_model(&self, _: bool) -> Option<String> {
            None
        }
        fn edit_mode(&self) -> &'static str {
            "Modeling"
        }
        fn parameters(
            &self,
            _: Option<&str>,
            _: Option<&str>,
            _: bool,
        ) -> Result<Vec<ParamInfo>, ApiError> {
            Ok(Vec::new())
        }
        fn groups(
            &self,
            _: Option<&str>,
            _: Option<&str>,
            _: bool,
        ) -> Result<Vec<(String, String)>, ApiError> {
            Ok(Vec::new())
        }
        fn values(&self, _: &str) -> Result<Vec<(String, f32)>, ApiError> {
            Err(ApiError::InvalidModel)
        }
        fn set_values(&mut self, _: &str, _: &[(String, f32)]) -> Result<(), ApiError> {
            Ok(())
        }
        fn clear_values(&mut self, _: &str) -> Result<(), ApiError> {
            Ok(())
        }
        fn log(&mut self, _: bool, _: &str, _: bool) {}
        fn register(
            &mut self,
            _: &str,
            _: Option<&str>,
            _: Option<&str>,
            _: Option<&str>,
        ) -> (String, bool) {
            ("tok".into(), true)
        }
        fn approved(&self, t: &str) -> bool {
            t == "tok"
        }
    }

    /// A WebSocket client on 127.0.0.1 gets answers (GetAPIVersion,
    /// RegisterPlugin, an error for bad JSON).
    #[test]
    fn round_trip_over_localhost() {
        // A free port.
        let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let mut server = Server::start(port).unwrap();
        let client = std::thread::spawn(move || {
            let stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
            let (mut ws, _) =
                tungstenite::client(format!("ws://127.0.0.1:{port}/"), stream).unwrap();
            let mut answers = Vec::new();
            for text in [
                r#"{"Version":"1.0.1","Timestamp":0,"RequestId":"a","Type":"Request","Method":"GetAPIVersion","Data":{}}"#,
                r#"{"Version":"1.0.1","Timestamp":0,"RequestId":"b","Type":"Request","Method":"RegisterPlugin","Data":{"Name":"t"}}"#,
                "not json",
            ] {
                ws.send(tungstenite::Message::text(text)).unwrap();
                loop {
                    if let tungstenite::Message::Text(t) = ws.read().unwrap() {
                        answers.push(t.to_string());
                        break;
                    }
                }
            }
            let _ = ws.close(None);
            answers
        });
        let mut host = Minimal;
        let t0 = Instant::now();
        while !client.is_finished() && t0.elapsed() < Duration::from_secs(10) {
            server.poll(&mut host, 1);
            std::thread::sleep(Duration::from_millis(5));
        }
        let answers = client.join().unwrap();
        assert!(
            answers[0].contains("\"Version\":\"1.0.1\"}")
                && answers[0].contains("\"RequestId\":\"a\""),
            "{}",
            answers[0]
        );
        assert!(answers[1].contains("\"Token\":\"tok\""), "{}", answers[1]);
        assert!(answers[2].contains("InvalidJson"), "{}", answers[2]);
    }
}
