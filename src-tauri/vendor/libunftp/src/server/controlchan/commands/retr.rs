//! The RFC 959 Retrieve (`RETR`) command
//
// This command causes the server-DTP to transfer a copy of the
// file, specified in the pathname, to the server- or user-DTP
// at the other end of the data connection.  The status and
// contents of the file at the server site shall be unaffected.

use crate::{
    auth::UserDetail,
    server::{
        ReplyCode,
        chancomms::DataChanCmd,
        controlchan::{
            Reply,
            command::Command,
            error::ControlChanError,
            handler::{CommandContext, CommandHandler},
        },
    },
    storage::{Metadata, StorageBackend},
};
use async_trait::async_trait;
use std::sync::Arc;

#[derive(Debug)]
pub struct Retr;

#[async_trait]
impl<Storage, User> CommandHandler<Storage, User> for Retr
where
    User: UserDetail + 'static,
    Storage: StorageBackend<User> + 'static,
    Storage::Metadata: Metadata,
{
    #[tracing_attributes::instrument]
    async fn handle(&self, args: CommandContext<Storage, User>) -> Result<Reply, ControlChanError> {
        let (cmd, path): (DataChanCmd, String) = match args.parsed_command.clone() {
            Command::Retr { path } => {
                let path_clone = path.clone();
                (DataChanCmd::Retr { path }, path_clone)
            }
            _ => panic!("Programmer error, expected command to be RETR"),
        };

        // PATCH Scan to Folder: controlla che il file esista PRIMA di rispondere 150.
        // Le multifunzione Ricoh fanno RETR di nome.pdf, nome-1.pdf, ... per trovare
        // un nome libero e si fermano al primo 550; con il 150 anticipato credevano
        // ogni nome occupato e non arrivavano mai allo STOR.
        let (storage, user, full_path) = {
            let session = args.session.lock().await;
            (Arc::clone(&session.storage), session.user.clone(), session.cwd.join(&path))
        };
        let exists = match (*user).as_ref() {
            Some(u) => storage.metadata(u, &full_path).await.is_ok(),
            None => true,
        };
        let mut session = args.session.lock().await;
        if !exists {
            // chiude la connessione dati gia' aperta (attivo) o in attesa (passivo)
            session.data_cmd_tx.take();
            if let Some(abort) = session.data_abort_tx.as_ref() {
                let _ = abort.try_send(());
            }
            slog::info!(args.logger, "RETR: file {:?} non presente, risposta 550", path);
            return Ok(Reply::new(ReplyCode::FileError, "File not found"));
        }

        let logger = args.logger;
        match session.data_cmd_tx.take() {
            Some(tx) => {
                tokio::spawn(async move {
                    if let Err(err) = tx.send(cmd).await {
                        slog::warn!(logger, "RETR: could not notify data channel to respond with RETR. {}", err);
                    }
                });
                Ok(Reply::new(ReplyCode::FileStatusOkay, "Sending data"))
            }
            None => {
                slog::warn!(logger, "RETR: no data connection established for RETRing {:?}", path);

                Ok(Reply::new(ReplyCode::CantOpenDataConnection, "No data connection established"))
            }
        }
    }
}
