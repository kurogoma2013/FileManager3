ALTER TABLE files ADD COLUMN uploaded_by INTEGER;
ALTER TABLE files ADD COLUMN deleted_by INTEGER;

CREATE INDEX idx_files_uploaded_by ON files(uploaded_by);
CREATE INDEX idx_files_deleted_by ON files(deleted_by);
