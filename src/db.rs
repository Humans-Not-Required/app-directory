use rusqlite::Connection;

pub fn init_db(path: &str) -> Connection {
    let conn = Connection::open(path).expect("Failed to open database");

    conn.execute_batch("PRAGMA journal_mode=WAL;")
        .expect("Failed to set WAL mode");

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS api_keys (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            key_hash TEXT NOT NULL UNIQUE,
            is_admin INTEGER NOT NULL DEFAULT 0,
            rate_limit INTEGER NOT NULL DEFAULT 100,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            revoked INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS apps (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            slug TEXT NOT NULL UNIQUE,
            short_description TEXT NOT NULL,
            description TEXT NOT NULL,
            homepage_url TEXT,
            api_url TEXT,
            api_spec_url TEXT,
            protocol TEXT NOT NULL DEFAULT 'rest',
            category TEXT NOT NULL DEFAULT 'other',
            tags TEXT NOT NULL DEFAULT '[]',
            logo_url TEXT,
            author_name TEXT NOT NULL,
            author_url TEXT,
            submitted_by_key_id TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending',
            avg_rating REAL NOT NULL DEFAULT 0.0,
            review_count INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now')),
            FOREIGN KEY (submitted_by_key_id) REFERENCES api_keys(id)
        );

        CREATE TABLE IF NOT EXISTS reviews (
            id TEXT PRIMARY KEY,
            app_id TEXT NOT NULL,
            reviewer_key_id TEXT NOT NULL,
            rating INTEGER NOT NULL CHECK(rating >= 1 AND rating <= 5),
            title TEXT,
            body TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            FOREIGN KEY (app_id) REFERENCES apps(id),
            FOREIGN KEY (reviewer_key_id) REFERENCES api_keys(id),
            UNIQUE(app_id, reviewer_key_id)
        );

        CREATE INDEX IF NOT EXISTS idx_apps_category ON apps(category);
        CREATE INDEX IF NOT EXISTS idx_apps_protocol ON apps(protocol);
        CREATE INDEX IF NOT EXISTS idx_apps_status ON apps(status);
        CREATE INDEX IF NOT EXISTS idx_apps_slug ON apps(slug);
        CREATE INDEX IF NOT EXISTS idx_reviews_app ON reviews(app_id);
        ",
    )
    .expect("Failed to initialize database");

    conn
}
