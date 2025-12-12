//! SQLite database operations for vector index

use rusqlite::{Connection, Result};
use std::path::Path;

/// Database schema version
const SCHEMA_VERSION: i32 = 2;

/// Initialize the vector index database
pub fn initialize_database(db_path: &Path) -> Result<()> {
    // Create parent directory if it doesn't exist
    if let Some(parent) = db_path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return Err(rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(e),
            ));
        }
    }

    let conn = Connection::open(db_path)?;

    // Create schema version table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER PRIMARY KEY
        )",
        [],
    )?;

    // Check current schema version
    let current_version: Option<i32> = conn
        .query_row(
            "SELECT version FROM schema_version ORDER BY version DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .ok();

    if current_version.is_none_or(|v| v < SCHEMA_VERSION) {
        // Apply schema updates
        apply_schema_updates(&conn)?;

        // Update schema version
        conn.execute(
            "INSERT OR REPLACE INTO schema_version (version) VALUES (?)",
            [SCHEMA_VERSION],
        )?;
    }

    Ok(())
}

/// Apply database schema updates
fn apply_schema_updates(conn: &Connection) -> Result<()> {
    // Get current schema version (default to 0 if no version table exists yet)
    let current_version: i32 = conn
        .query_row(
            "SELECT version FROM schema_version ORDER BY version DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    // Apply migrations based on current version
    if current_version < 1 {
        // Schema version 1: Initial schema
        conn.execute(
            "CREATE TABLE IF NOT EXISTS skills (
                id TEXT PRIMARY KEY,
                skill_path TEXT NOT NULL,
                frontmatter_json TEXT NOT NULL,
                embedding_json TEXT NOT NULL,
                file_hash TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
            [],
        )?;

        // Create indexes for better performance
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_updated_at ON skills(updated_at)",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_file_hash ON skills(file_hash)",
            [],
        )?;
    }

    if current_version < 2 {
        // Schema version 2: Add source tracking columns
        conn.execute("ALTER TABLE skills ADD COLUMN source_url TEXT", [])?;

        conn.execute("ALTER TABLE skills ADD COLUMN source_type TEXT", [])?;

        conn.execute("ALTER TABLE skills ADD COLUMN source_branch TEXT", [])?;

        conn.execute("ALTER TABLE skills ADD COLUMN source_tag TEXT", [])?;

        conn.execute("ALTER TABLE skills ADD COLUMN source_subdir TEXT", [])?;

        conn.execute("ALTER TABLE skills ADD COLUMN installed_from TEXT", [])?;

        conn.execute("ALTER TABLE skills ADD COLUMN version TEXT", [])?;

        conn.execute("ALTER TABLE skills ADD COLUMN commit_hash TEXT", [])?;

        conn.execute("ALTER TABLE skills ADD COLUMN fetched_at TEXT", [])?;

        conn.execute("ALTER TABLE skills ADD COLUMN editable INTEGER", [])?;

        // Create indexes for the new columns
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_source_url ON skills(source_url)",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_source_type ON skills(source_type)",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_installed_from ON skills(installed_from)",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_fetched_at ON skills(fetched_at)",
            [],
        )?;
    }

    Ok(())
}

/// Database connection wrapper with proper error handling
pub struct VectorIndexConnection {
    conn: Connection,
}

impl VectorIndexConnection {
    /// Open a database connection and ensure schema is initialized
    pub fn open(db_path: &Path) -> Result<Self> {
        initialize_database(db_path)?;
        let conn = Connection::open(db_path)?;
        Ok(Self { conn })
    }

    /// Get a reference to the underlying connection
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Get a mutable reference to the underlying connection
    pub fn conn_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_database_initialization() {
        let temp_file = NamedTempFile::new().unwrap();
        let db_path = temp_file.path();

        // Initialize database
        initialize_database(db_path).unwrap();

        // Verify schema was created
        let conn = Connection::open(db_path).unwrap();

        // Check if skills table exists
        let table_count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='skills'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(table_count, 1);

        // Check if schema_version table exists
        let version_count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='schema_version'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(version_count, 1);
    }
}
