use crate::cli;
use crate::log;
// The std lib uses hashbrown internally, so there should be no "extra" costs in
// using it as the HashMap implementation, and it allows us to remove dependency
// on nightly.
//
// See https://doc.rust-lang.org/src/std/collections/hash/map.rs.html#5-7
//
use hashbrown::HashMap;

pub struct Socket {
    socket: std::net::UdpSocket,
    clients: std::sync::Arc<std::sync::Mutex<HashMap<std::net::SocketAddr, std::time::SystemTime>>>,
}

pub fn new(address: &str) -> Result<Socket, std::io::Error> {
    let socket = std::net::UdpSocket::bind(address)?;
    socket.set_read_timeout(Some(std::time::Duration::from_micros(100)))?;
    Ok(Socket {
        socket,
        clients: std::sync::Arc::new(std::sync::Mutex::new(Default::default())),
    })
}

impl Socket {
    fn remove_old_clients(&self) {
        let old_clients: HashMap<std::net::SocketAddr, std::time::SystemTime> = self
            .clients
            .lock()
            .unwrap()
            .drain_filter(|_client, time| {
                std::time::SystemTime::now()
                    .duration_since(*time)
                    .unwrap()
                    .as_secs()
                    > 10
            })
            .collect();

        if cli::is_verbose() && !old_clients.is_empty() {
            log!("Removing old clients");
            old_clients.iter().for_each(|(client, time)| {
                log!(
                    "Removing client: {}, with last message from: {:?}",
                    client,
                    time
                );
            })
        }
    }

    fn remove_old_clients_by_max_number(&self) {
        let max_client_number = cli::options().udp_max_clients_number;
        if self.clients.lock().unwrap().len() <= max_client_number {
            return;
        }

        // Create a vector with (time, socket) to sort by time and remove old clients
        let mut times_clients: Vec<(std::time::SystemTime, std::net::SocketAddr)> = self
            .clients
            .lock()
            .unwrap()
            .iter()
            .map(|(socket, time)| (time.clone(), socket.clone()))
            .collect();

        // Newer first
        times_clients.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        times_clients.truncate(max_client_number);
        let final_sockets: Vec<&std::net::SocketAddr> =
            times_clients.iter().map(|(_, socket)| socket).collect();

        let old_clients: HashMap<std::net::SocketAddr, std::time::SystemTime> = self
            .clients
            .lock()
            .unwrap()
            .drain_filter(|socket, _| !final_sockets.contains(&socket))
            .collect();

        if cli::is_verbose() && !old_clients.is_empty() {
            log!("Removing old clients by maximum number");
            old_clients.iter().for_each(|(client, time)| {
                log!(
                    "Removing client: {}, with last message from: {:?}",
                    client,
                    time
                );
            })
        }
    }

    pub fn write(&self, data: &[u8]) {
        if cli::options().no_udp_disconnection {
            self.remove_old_clients();
        }

        // Make sure that we are not going to have an infinity amount of clients!
        self.remove_old_clients_by_max_number();

        for client in self.clients.lock().unwrap().keys() {
            log!("W {} : {:?}", client, data);
            if let Err(error) = self.socket.send_to(data, client) {
                println!(
                    "Error while writing in UDP: {:?} for client: {}",
                    error, client
                );
            }
        }
    }

    pub fn read(&self) -> Vec<u8> {
        let mut buffer = vec![0; 4096];
        let mut data = vec![];
        while let Ok((size, client)) = self.socket.recv_from(&mut buffer) {
            let now = std::time::SystemTime::now();
            if cli::is_verbose() && !self.clients.lock().unwrap().contains_key(&client) {
                log!("Adding new client: {}, message in {:?}", client, now)
            }

            log!("R {} : {:?}", client, &buffer[..size]);

            self.clients.lock().unwrap().insert(client, now);
            data.extend_from_slice(&buffer[..size]);
        }
        return data;
    }
}
