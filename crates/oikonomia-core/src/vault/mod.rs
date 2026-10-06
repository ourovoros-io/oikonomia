//! Encrypted vault: Argon2id key derivation + `SQLCipher` database.

mod backup;
mod crypto;
mod files;
mod header;
mod paths;
mod permissions;
mod store;

pub use backup::{BACKUP_EXTENSION, backup_to_path, default_backup_file_name, restore_from_path};
pub use header::VaultHeader;
pub use paths::{default_data_dir, vault_db_path, vault_header_path};
pub use store::{Vault, VaultStatus};
