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
            is_featured INTEGER NOT NULL DEFAULT 0,
            is_verified INTEGER NOT NULL DEFAULT 0,
            avg_rating REAL NOT NULL DEFAULT 0.0,
            review_count INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now')),
            FOREIGN KEY (submitted_by_key_id) REFERENCES api_keys(id)
        );

        CREATE TABLE IF NOT EXISTS reviews (
            id TEXT PRIMARY KEY,
            app_id TEXT NOT NULL,
            reviewer_key_id TEXT,
            reviewer_name TEXT,
            rating INTEGER NOT NULL CHECK(rating >= 1 AND rating <= 5),
            title TEXT,
            body TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            FOREIGN KEY (app_id) REFERENCES apps(id)
        );

        CREATE TABLE IF NOT EXISTS health_checks (
            id TEXT PRIMARY KEY,
            app_id TEXT NOT NULL,
            status TEXT NOT NULL,
            status_code INTEGER,
            response_time_ms INTEGER,
            error_message TEXT,
            checked_url TEXT NOT NULL,
            checked_at TEXT NOT NULL DEFAULT (datetime('now')),
            FOREIGN KEY (app_id) REFERENCES apps(id)
        );

        CREATE TABLE IF NOT EXISTS webhooks (
            id TEXT PRIMARY KEY,
            url TEXT NOT NULL,
            secret TEXT NOT NULL,
            events TEXT NOT NULL DEFAULT '[]',
            created_by TEXT NOT NULL,
            active INTEGER NOT NULL DEFAULT 1,
            failure_count INTEGER NOT NULL DEFAULT 0,
            last_triggered_at TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            FOREIGN KEY (created_by) REFERENCES api_keys(id)
        );

        CREATE INDEX IF NOT EXISTS idx_apps_category ON apps(category);
        CREATE INDEX IF NOT EXISTS idx_apps_protocol ON apps(protocol);
        CREATE INDEX IF NOT EXISTS idx_apps_status ON apps(status);
        CREATE INDEX IF NOT EXISTS idx_apps_slug ON apps(slug);
        CREATE INDEX IF NOT EXISTS idx_reviews_app ON reviews(app_id);
        CREATE UNIQUE INDEX IF NOT EXISTS idx_reviews_unique_key
            ON reviews(app_id, reviewer_key_id) WHERE reviewer_key_id IS NOT NULL;
        CREATE INDEX IF NOT EXISTS idx_health_checks_app ON health_checks(app_id);
        CREATE INDEX IF NOT EXISTS idx_health_checks_checked_at ON health_checks(checked_at);

        CREATE TABLE IF NOT EXISTS app_views (
            id TEXT PRIMARY KEY,
            app_id TEXT NOT NULL,
            viewer_key_id TEXT NOT NULL,
            viewed_at TEXT NOT NULL DEFAULT (datetime('now')),
            FOREIGN KEY (app_id) REFERENCES apps(id)
        );

        CREATE INDEX IF NOT EXISTS idx_app_views_app ON app_views(app_id);
        CREATE INDEX IF NOT EXISTS idx_app_views_viewed_at ON app_views(viewed_at);
        CREATE INDEX IF NOT EXISTS idx_app_views_app_viewed ON app_views(app_id, viewed_at);
        ",
    )
    .expect("Failed to initialize database");

    // Migration: add badge columns if missing (for existing databases)
    let has_featured: bool = conn.prepare("SELECT is_featured FROM apps LIMIT 0").is_ok();
    if !has_featured {
        conn.execute_batch(
            "ALTER TABLE apps ADD COLUMN is_featured INTEGER NOT NULL DEFAULT 0;
             ALTER TABLE apps ADD COLUMN is_verified INTEGER NOT NULL DEFAULT 0;",
        )
        .expect("Failed to add badge columns");
    }

    // Migration: add health check columns if missing
    let has_health: bool = conn
        .prepare("SELECT last_health_status FROM apps LIMIT 0")
        .is_ok();
    if !has_health {
        conn.execute_batch(
            "ALTER TABLE apps ADD COLUMN last_health_status TEXT;
             ALTER TABLE apps ADD COLUMN last_checked_at TEXT;
             ALTER TABLE apps ADD COLUMN uptime_pct REAL;",
        )
        .expect("Failed to add health check columns");
    }

    // Migration: add approval workflow columns if missing
    let has_review_note: bool = conn.prepare("SELECT review_note FROM apps LIMIT 0").is_ok();
    if !has_review_note {
        conn.execute_batch(
            "ALTER TABLE apps ADD COLUMN review_note TEXT;
             ALTER TABLE apps ADD COLUMN reviewed_by TEXT;
             ALTER TABLE apps ADD COLUMN reviewed_at TEXT;",
        )
        .expect("Failed to add approval workflow columns");
    }

    // Migration: add deprecation workflow columns if missing
    let has_deprecated_reason: bool = conn
        .prepare("SELECT deprecated_reason FROM apps LIMIT 0")
        .is_ok();
    if !has_deprecated_reason {
        conn.execute_batch(
            "ALTER TABLE apps ADD COLUMN deprecated_reason TEXT;
             ALTER TABLE apps ADD COLUMN deprecated_by TEXT;
             ALTER TABLE apps ADD COLUMN deprecated_at TEXT;
             ALTER TABLE apps ADD COLUMN replacement_app_id TEXT;
             ALTER TABLE apps ADD COLUMN sunset_at TEXT;",
        )
        .expect("Failed to add deprecation workflow columns");
    }

    // Migration: add edit_token_hash for per-resource auth
    let has_edit_token: bool = conn.prepare("SELECT edit_token_hash FROM apps LIMIT 0").is_ok();
    if !has_edit_token {
        conn.execute_batch(
            "ALTER TABLE apps ADD COLUMN edit_token_hash TEXT;",
        )
        .expect("Failed to add edit_token_hash column");
    }

    // Migration: make submitted_by_key_id nullable for anonymous submissions
    // SQLite doesn't support ALTER COLUMN, so we need to recreate the table if needed
    // Check if the column is still NOT NULL by trying to insert a null value
    let is_nullable = conn.execute(
        "INSERT INTO apps (id, name, slug, short_description, description, author_name, protocol, category, submitted_by_key_id) VALUES ('test_null_check', 'Test', 'test', 'Test', 'Test', 'Test', 'rest', 'other', NULL)",
        [],
    ).is_ok();
    
    // Clean up test row if it was inserted
    let _ = conn.execute("DELETE FROM apps WHERE id = 'test_null_check'", []);

    if !is_nullable {
        // Need to migrate the table to make submitted_by_key_id nullable
        println!("⚠️  Migrating apps table to make submitted_by_key_id nullable...");
        conn.execute_batch(
            "BEGIN TRANSACTION;
             
             CREATE TABLE apps_new (
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
                submitted_by_key_id TEXT,
                status TEXT NOT NULL DEFAULT 'pending',
                is_featured INTEGER NOT NULL DEFAULT 0,
                is_verified INTEGER NOT NULL DEFAULT 0,
                avg_rating REAL NOT NULL DEFAULT 0.0,
                review_count INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                last_health_status TEXT,
                last_checked_at TEXT,
                uptime_pct REAL,
                review_note TEXT,
                reviewed_by TEXT,
                reviewed_at TEXT,
                deprecated_reason TEXT,
                deprecated_by TEXT,
                deprecated_at TEXT,
                replacement_app_id TEXT,
                sunset_at TEXT,
                edit_token_hash TEXT
             );
             
             INSERT INTO apps_new SELECT * FROM apps;
             DROP TABLE apps;
             ALTER TABLE apps_new RENAME TO apps;
             
             CREATE INDEX idx_apps_category ON apps(category);
             CREATE INDEX idx_apps_protocol ON apps(protocol);
             CREATE INDEX idx_apps_status ON apps(status);
             CREATE INDEX idx_apps_slug ON apps(slug);
             
             COMMIT;",
        )
        .expect("Failed to migrate apps table");
        println!("✓ Migration complete");
    }

    // Migration: fix reviews table — remove broken FK on reviewer_key_id,
    // make nullable for anonymous reviews, add reviewer_name field
    let has_reviewer_name: bool = conn
        .prepare("SELECT reviewer_name FROM reviews LIMIT 0")
        .is_ok();
    if !has_reviewer_name {
        println!("⚠️  Migrating reviews table (nullable reviewer_key_id, add reviewer_name)...");
        conn.execute_batch(
            "BEGIN TRANSACTION;

             CREATE TABLE reviews_new (
                id TEXT PRIMARY KEY,
                app_id TEXT NOT NULL,
                reviewer_key_id TEXT,
                reviewer_name TEXT,
                rating INTEGER NOT NULL CHECK(rating >= 1 AND rating <= 5),
                title TEXT,
                body TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                FOREIGN KEY (app_id) REFERENCES apps(id)
             );

             INSERT INTO reviews_new (id, app_id, reviewer_key_id, rating, title, body, created_at)
                SELECT id, app_id, reviewer_key_id, rating, title, body, created_at FROM reviews;

             DROP TABLE reviews;
             ALTER TABLE reviews_new RENAME TO reviews;

             CREATE INDEX idx_reviews_app ON reviews(app_id);
             CREATE UNIQUE INDEX IF NOT EXISTS idx_reviews_unique_key
                ON reviews(app_id, reviewer_key_id) WHERE reviewer_key_id IS NOT NULL;

             COMMIT;",
        )
        .expect("Failed to migrate reviews table");
        println!("✓ Reviews migration complete");
    }

    conn
}
