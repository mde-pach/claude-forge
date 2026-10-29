//! The hooks' quick questions to the types engine. Starting it is the checker's
//! (`forge_check::engine::start`, run by the detached `forge engine start` after SessionStart).

use std::time::Duration;

use forge_engine::Client;
use forge_home::{Home, RepoId};

/// The client of the repository's engine when one is listening; never starts it.
pub fn running(home: &Home, id: &RepoId, timeout: Duration) -> Option<Client> {
    let socket = home.engine_socket(id);
    forge_engine::socket_addr(&socket)
        .exists()
        .then(|| Client::new(&socket).with_timeout(timeout))
}
