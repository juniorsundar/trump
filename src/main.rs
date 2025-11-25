use std::net::TcpStream;
use std::io::Read;
use ssh2::Session;

fn main() {
    let tcp = TcpStream::connect("192.168.1.250:22").unwrap();
    let mut session = Session::new().unwrap();
    session.set_tcp_stream(tcp);
    session.handshake().unwrap();
    session.userauth_password("juniorsundar", "Rjunu021097").unwrap();

    let mut channel = session.channel_session().unwrap();
    channel.exec("ls -lah").unwrap();

    let mut output = String::new();
    channel.read_to_string(&mut output).unwrap();
    println!("{}", output);

    channel.wait_close().unwrap();
    println!("Exit status: {}", channel.exit_status().unwrap());
}
