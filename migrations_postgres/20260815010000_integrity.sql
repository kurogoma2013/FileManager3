-- Keep dealer contacts tied to the dealer name used by the application.
CREATE TABLE dealer_contacts_new (
    id BIGSERIAL PRIMARY KEY,
    dealer_name TEXT NOT NULL REFERENCES dealers(name) ON UPDATE CASCADE ON DELETE CASCADE,
    name TEXT NOT NULL,
    phone TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at TEXT
);

INSERT INTO dealer_contacts_new (id, dealer_name, name, phone, created_at, deleted_at)
SELECT id, dealer_name, name, phone, created_at, deleted_at
FROM dealer_contacts;

DROP TABLE dealer_contacts;
ALTER TABLE dealer_contacts_new RENAME TO dealer_contacts;

CREATE INDEX idx_dealer_contacts_name_active
    ON dealer_contacts(dealer_name, name, deleted_at);

-- Files are represented once per project, so file_hash cannot be globally unique.
-- This composite index supports the shared-storage lookup and active-row filter.
CREATE INDEX idx_files_hash_active ON files(file_hash, deleted_at);
