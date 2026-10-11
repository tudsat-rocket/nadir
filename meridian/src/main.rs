use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "\
meridian - shares MAVLink telemetry sessions over HTTP

USAGE:
    meridian [--listen <addr>] [--data <dir>]

OPTIONS:
    -l, --listen <addr>  Address to serve on. Default: 0.0.0.0:7654
    -d, --data <dir>     Directory holding one <id>.tlog per session. Default: sessions
    -h, --help           Print this help.

ENDPOINTS:
    GET  /sessions                 One line per session: id, bytes, seconds idle.
    GET  /sessions/<id>            The session as a .tlog, staying open while live.
         [?offset=<n>&epoch=<e>]   Resume at byte <n> of the session <e> named in
                                   x-session-epoch, or from the start of a newer one.
    POST /sessions/<id>?offset=<n> Append records if <n> is the current length,
                                   else 409 with the current length.

There is no authentication or TLS: serve on a trusted network only.
";

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let mut listen: SocketAddr = ([0, 0, 0, 0], 7654).into();
    let mut data = PathBuf::from("sessions");

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "-l" | "--listen" => match args.next().map(|addr| addr.parse()) {
                Some(Ok(addr)) => listen = addr,
                _ => {
                    eprintln!("meridian: --listen needs an address like 0.0.0.0:7654");
                    return ExitCode::FAILURE;
                }
            },
            "-d" | "--data" => match args.next() {
                Some(dir) => data = dir.into(),
                None => {
                    eprintln!("meridian: --data needs a directory");
                    return ExitCode::FAILURE;
                }
            },
            _ => {
                eprint!("meridian: unknown argument `{arg}`\n\n{USAGE}");
                return ExitCode::FAILURE;
            }
        }
    }

    let listener = match tokio::net::TcpListener::bind(listen).await {
        Ok(listener) => listener,
        Err(e) => {
            tracing::error!("Failed to listen on {listen}: {e}");
            return ExitCode::FAILURE;
        }
    };

    tracing::info!("serving http://{listen}/sessions from {}", data.display());

    match meridian::serve(listener, data).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!("{e}");
            ExitCode::FAILURE
        }
    }
}
