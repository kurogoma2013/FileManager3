use std::net::UdpSocket;

pub fn local_lan_ip() -> Option<String> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    Some(socket.local_addr().ok()?.ip().to_string())
}

pub fn local_host_name() -> Option<String> {
    let output = std::process::Command::new("hostname").output().ok()?;
    let hostname = String::from_utf8(output.stdout)
        .ok()?
        .trim()
        .to_ascii_lowercase();
    if hostname.is_empty() {
        return None;
    }
    if hostname.contains('.') {
        Some(hostname)
    } else {
        Some(format!("{hostname}.local"))
    }
}

pub fn webui_urls(host: &str, port: u16, _secure: bool) -> Vec<String> {
    let scheme = "https";
    if host == "0.0.0.0" {
        let mut urls = vec![format!("{scheme}://127.0.0.1:{port}")];
        if let Some(ip) = local_lan_ip() {
            urls.push(format!("{scheme}://{ip}:{port}"));
        }
        if let Some(hostname) = local_host_name() {
            urls.push(format!("{scheme}://{hostname}:{port}"));
        }
        urls
    } else {
        vec![format!("{scheme}://{host}:{port}")]
    }
}
