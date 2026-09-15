import Foundation

struct CachedProjectRecord: Codable {
    let detail: ProjectDetail
    let fileIDs: [Int64]
    let serverKey: String
    let savedAt: Date
    var expiresAt: Date
}

final class OfflineCache {
    private let fileManager = FileManager.default
    private let root: URL
    private let defaults = UserDefaults.standard

    init() {
        let appSupport = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        root = appSupport.appendingPathComponent("FileManager3Offline", isDirectory: true)
        try? FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        cleanupExpired()
    }

    var retentionDays: Int { max(1, min(3650, defaults.integer(forKey: "offlineRetentionDays") == 0 ? 30 : defaults.integer(forKey: "offlineRetentionDays"))) }
    func setRetentionDays(_ days: Int) { defaults.set(max(1, min(3650, days)), forKey: "offlineRetentionDays") }

    func save(detail: ProjectDetail, fileData: [Int64: Data], serverURL: String) throws {
        let serverKey = key(serverURL), projectID = detail.project.id, directory = projectDirectory(serverKey: serverKey, projectID: projectID)
        try fileManager.createDirectory(at: directory, withIntermediateDirectories: true)
        for (fileID, data) in fileData { try data.write(to: directory.appendingPathComponent("\(fileID).bin"), options: .atomic) }
        let existing = loadRecord(projectID: projectID, serverKey: serverKey), ids = Set((existing?.fileIDs ?? []) + Array(fileData.keys))
        let now = Date(), record = CachedProjectRecord(detail: detail, fileIDs: Array(ids), serverKey: serverKey, savedAt: now, expiresAt: now.addingTimeInterval(TimeInterval(retentionDays) * 86400))
        let encoder = JSONEncoder(); encoder.dateEncodingStrategy = .millisecondsSince1970
        try encoder.encode(record).write(to: recordURL(serverKey: serverKey, projectID: projectID), options: .atomic)
    }

    func load(projectID: Int64, serverURL: String) -> ProjectDetail? {
        cleanupExpired(); let serverKey = key(serverURL), url = recordURL(serverKey: serverKey, projectID: projectID), decoder = JSONDecoder(); decoder.dateDecodingStrategy = .millisecondsSince1970
        guard let data = try? Data(contentsOf: url), let record = try? decoder.decode(CachedProjectRecord.self, from: data), record.expiresAt > Date() else { return nil }
        let available = Set(record.fileIDs.filter { fileManager.fileExists(atPath: fileURL(serverKey: serverKey, projectID: projectID, fileID: $0).path) })
        let documents = record.detail.documents.filter { available.contains($0.id) }, pictures = record.detail.pictures.filter { available.contains($0.id) }
        return ProjectDetail(project: record.detail.project, documents: documents, pictures: pictures)
    }

    func cachedProjects(serverURL: String) -> [Project] {
        cleanupExpired(); let serverKey = key(serverURL), directory = root.appendingPathComponent(serverKey, isDirectory: true)
        guard let names = try? fileManager.contentsOfDirectory(atPath: directory.path) else { return [] }
        let decoder = JSONDecoder(); decoder.dateDecodingStrategy = .millisecondsSince1970
        return names.compactMap { name in guard let id = Int64(name.replacingOccurrences(of: ".json", with: "")), let data = try? Data(contentsOf: directory.appendingPathComponent(name)), let record = try? decoder.decode(CachedProjectRecord.self, from: data), record.expiresAt > Date() else { return nil }; return record.detail.project }
    }

    func fileURL(projectID: Int64, fileID: Int64, serverURL: String) -> URL { fileURL(serverKey: key(serverURL), projectID: projectID, fileID: fileID) }
    func updateExpiration(projectID: Int64, serverURL: String) {
        let serverKey = key(serverURL), url = recordURL(serverKey: serverKey, projectID: projectID), decoder = JSONDecoder(); decoder.dateDecodingStrategy = .millisecondsSince1970
        guard let data = try? Data(contentsOf: url), var record = try? decoder.decode(CachedProjectRecord.self, from: data) else { return }
        record.expiresAt = Date().addingTimeInterval(TimeInterval(retentionDays) * 86400); let encoder = JSONEncoder(); encoder.dateEncodingStrategy = .millisecondsSince1970; try? encoder.encode(record).write(to: url, options: .atomic)
    }

    func clearAll() { try? fileManager.removeItem(at: root); try? fileManager.createDirectory(at: root, withIntermediateDirectories: true) }

    private func loadRecord(projectID: Int64, serverKey: String) -> CachedProjectRecord? {
        let decoder = JSONDecoder(); decoder.dateDecodingStrategy = .millisecondsSince1970
        guard let data = try? Data(contentsOf: recordURL(serverKey: serverKey, projectID: projectID)) else { return nil }
        return try? decoder.decode(CachedProjectRecord.self, from: data)
    }

    private func cleanupExpired() {
        guard let servers = try? fileManager.contentsOfDirectory(at: root, includingPropertiesForKeys: nil) else { return }
        let decoder = JSONDecoder(); decoder.dateDecodingStrategy = .millisecondsSince1970
        for server in servers { guard let records = try? fileManager.contentsOfDirectory(at: server, includingPropertiesForKeys: nil) else { continue }; for recordURL in records where recordURL.pathExtension == "json" { guard let data = try? Data(contentsOf: recordURL), let record = try? decoder.decode(CachedProjectRecord.self, from: data) else { try? fileManager.removeItem(at: recordURL); continue }; if record.expiresAt <= Date() { try? fileManager.removeItem(at: recordURL); try? fileManager.removeItem(at: recordURL.deletingPathExtension()) } } }
    }

    private func key(_ value: String) -> String { Data(value.trimmingCharacters(in: .whitespacesAndNewlines).utf8).base64EncodedString().replacingOccurrences(of: "/", with: "_").replacingOccurrences(of: "+", with: "-").replacingOccurrences(of: "=", with: "") }
    private func projectDirectory(serverKey: String, projectID: Int64) -> URL { root.appendingPathComponent(serverKey, isDirectory: true).appendingPathComponent("\(projectID)", isDirectory: true) }
    private func recordURL(serverKey: String, projectID: Int64) -> URL { root.appendingPathComponent(serverKey, isDirectory: true).appendingPathComponent("\(projectID).json") }
    private func fileURL(serverKey: String, projectID: Int64, fileID: Int64) -> URL { projectDirectory(serverKey: serverKey, projectID: projectID).appendingPathComponent("\(fileID).bin") }
}
