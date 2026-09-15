import SwiftUI
import UIKit
import UniformTypeIdentifiers
import QuickLook

@main
struct FileManager3IOSApp: App {
    @StateObject private var model = AppModel()
    var body: some Scene { WindowGroup { RootView().environmentObject(model) } }
}

@MainActor
final class AppModel: ObservableObject {
    @Published var baseURL = "https://mbpm5.local:3443"
    @Published var username = ""
    @Published var password = ""
    @Published var projects: [Project] = []
    @Published var selectedProject: ProjectDetail?
    @Published var errorMessage = ""
    @Published var isLoading = false
    @Published var offlineMode = false
    @Published var role = ""
    private let api = APIClient()
    private let offlineCache = OfflineCache()
    var retentionDays: Int { offlineCache.retentionDays }

    func login() { isLoading = true; errorMessage = ""; offlineMode = false; api.baseURL = baseURL.trimmingCharacters(in: CharacterSet(charactersIn: "/")); Task { do { let me = try await api.login(username: username, password: password); role = me.role; try await search() } catch { errorMessage = error.localizedDescription }; isLoading = false } }
    func search(number: String = "", name: String = "", address: String = "", sort: String = "updated_desc") async throws { do { projects = try await api.search(number: number, name: name, address: address, sort: sort); offlineMode = false } catch { let cached = offlineCache.cachedProjects(serverURL: baseURL); guard !cached.isEmpty else { throw error }; projects = cached; offlineMode = true } }
    func showOfflineProjects() { let cached = offlineCache.cachedProjects(serverURL: baseURL); if cached.isEmpty { errorMessage = "保存済み案件がありません。"; return }; projects = cached; offlineMode = true }
    func open(_ project: Project) { Task { do { selectedProject = try await api.detail(id: project.id); offlineMode = false } catch { if let cached = offlineCache.load(projectID: project.id, serverURL: baseURL) { selectedProject = cached; offlineMode = true } else { errorMessage = error.localizedDescription } } } }
    func upload(_ url: URL, category: String) { guard let project = selectedProject, !offlineMode else { return }; Task { do { try await api.upload(projectID: project.project.id, fileURL: url, category: category); selectedProject = try await api.detail(id: project.project.id) } catch { errorMessage = error.localizedDescription } } }
    func updateTag(fileID: Int64, tag: String) { guard let detail = selectedProject, !offlineMode else { return }; Task { do { let updated = try await api.updateTag(fileID: fileID, tag: tag); guard let current = selectedProject else { return }; let pictures = current.pictures.map { $0.id == updated.id ? updated : $0 }; selectedProject = ProjectDetail(project: detail.project, documents: current.documents, pictures: pictures) } catch { errorMessage = error.localizedDescription } } }
    var canManage: Bool { role == "admin" || role == "member" }
    func openManagementPage(_ path: String) { guard let url = URL(string: baseURL + path) else { return }; UIApplication.shared.open(url) }
    func cacheSelectedFiles(_ files: [FileItem]) { guard let detail = selectedProject, !offlineMode, !files.isEmpty else { errorMessage = "オンライン時に保存するファイルを選択してください。"; return }; Task { do { var data: [Int64: Data] = [:]; for file in files { data[file.id] = try await api.download(fileID: file.id) }; try offlineCache.save(detail: detail, fileData: data, serverURL: baseURL); errorMessage = "選択したファイルをオフライン表示用に保存しました。" } catch { errorMessage = error.localizedDescription } } }
    func setRetentionDays(_ value: String) { guard let days = Int(value), (1...3650).contains(days) else { errorMessage = "保存期間は1〜3650日で指定してください。"; return }; offlineCache.setRetentionDays(days); if let project = selectedProject { offlineCache.updateExpiration(projectID: project.project.id, serverURL: baseURL) }; errorMessage = "保存期間を更新しました。" }
    func cachedFileURL(_ file: FileItem) -> URL? { guard let project = selectedProject else { return nil }; let url = offlineCache.fileURL(projectID: project.project.id, fileID: file.id, serverURL: baseURL); return FileManager.default.fileExists(atPath: url.path) ? url : nil }
    func thumbnail(for file: FileItem) async -> UIImage? { do { let data: Data; if offlineMode, let url = cachedFileURL(file) { data = try Data(contentsOf: url) } else { data = try await api.download(fileID: file.id) }; return UIImage(data: data) } catch { return nil } }
    func previewURL(for file: FileItem) async -> URL? { if let cached = cachedFileURL(file) { return cached }; do { let data = try await api.download(fileID: file.id); let ext = URL(fileURLWithPath: file.filePath).pathExtension; let name = UUID().uuidString + (ext.isEmpty ? "" : ".\(ext)"); let url = FileManager.default.temporaryDirectory.appendingPathComponent(name); try data.write(to: url); return url } catch { return nil } }
    func clearOfflineCache() { offlineCache.clearAll(); errorMessage = "オフラインキャッシュを削除しました。" }
    func logout() { api.logout(); projects = []; selectedProject = nil; username = ""; password = ""; role = ""; offlineMode = false }
}

struct RootView: View {
    @EnvironmentObject private var model: AppModel
    var body: some View { Group { if model.selectedProject != nil { DetailView() } else if model.projects.isEmpty && model.username.isEmpty { LoginView() } else { SearchView() } }.alert("エラー", isPresented: Binding(get: { !model.errorMessage.isEmpty }, set: { if !$0 { model.errorMessage = "" } })) { Button("閉じる", role: .cancel) {} } message: { Text(model.errorMessage) } }
}

struct LoginView: View {
    @EnvironmentObject private var model: AppModel
    var body: some View { NavigationStack { Form { Section("FileManager3") { TextField("サーバーURL", text: $model.baseURL).textInputAutocapitalization(.never).keyboardType(.URL); TextField("ユーザー名", text: $model.username).textInputAutocapitalization(.never); SecureField("パスワード", text: $model.password); Button(model.isLoading ? "接続中…" : "ログイン") { model.login() }.disabled(model.isLoading || model.username.isEmpty || model.password.isEmpty); Button("保存済み案件をオフライン表示") { model.showOfflineProjects() }; Button("オフラインキャッシュを削除", role: .destructive) { model.clearOfflineCache() } }; Section { Text("iPhone実機ではMacのLAN IPまたはHTTPSホスト名を指定してください。Basic認証は使用しません。").font(.footnote).foregroundStyle(.secondary) } }.navigationTitle("FileManager3") } }
}

struct SearchView: View {
    @EnvironmentObject private var model: AppModel
    @State private var number = ""; @State private var name = ""; @State private var address = ""; @State private var sort = "updated_desc"
    private let sortOptions = [("updated_desc", "更新日時の新しい順"), ("updated_asc", "更新日時の古い順"), ("name_asc", "案件名順（昇順）"), ("name_desc", "案件名順（降順）"), ("number_asc", "案件番号順（昇順）"), ("number_desc", "案件番号順（降順）")]
    var body: some View {
        NavigationStack {
            List {
                if model.offlineMode { Section { Text("オフライン表示中（読み取り専用）").foregroundStyle(.orange) } }
                Section("検索条件") {
                    TextField("案件番号", text: $number).disabled(model.offlineMode)
                    TextField("案件名", text: $name).disabled(model.offlineMode)
                    TextField("住所", text: $address).disabled(model.offlineMode)
                    Picker("並び順", selection: $sort) { ForEach(sortOptions, id: \.0) { Text($0.1).tag($0.0) } }.disabled(model.offlineMode)
                    Button("検索") { Task { do { try await model.search(number: number, name: name, address: address, sort: sort) } catch { model.errorMessage = error.localizedDescription } } }.disabled(model.offlineMode)
                }
                if model.canManage { Section { NavigationLink("管理メニュー") { ManagementView() } } }
                Section("案件一覧") {
                    ForEach(model.projects) { project in
                        Button { model.open(project) } label: { VStack(alignment: .leading, spacing: 5) { Text("\(project.projectNumber)  \(project.name)").font(.headline); Text(project.address).font(.subheadline).foregroundStyle(.secondary); Text("更新日時: \(project.updatedAt)").font(.caption).foregroundStyle(.secondary) } }
                    }
                }
                Section { Button("ログアウト", role: .destructive) { model.logout() } }
            }
            .navigationTitle("案件検索")
        }
    }
}

struct ManagementView: View {
    @EnvironmentObject private var model: AppModel
    var body: some View {
        List {
            Section("管理メニュー") {
                Button("案件管理（ブラウザで開く）") { model.openManagementPage("/admin/projects") }
                Button("販売店管理（ブラウザで開く）") { model.openManagementPage("/admin/dealers") }
                if model.role == "admin" {
                    Button("ユーザー管理（ブラウザで開く）") { model.openManagementPage("/admin/users") }
                    Button("仕様書（ブラウザで開く）") { model.openManagementPage("/admin/specifications") }
                }
            }
        }
        .navigationTitle("管理メニュー")
    }
}

struct DetailView: View {
    @EnvironmentObject private var model: AppModel
    @State private var pickerPresented = false
    @State private var selectedFileIDs: Set<Int64> = []
    @State private var retentionDays = "30"
    @State private var previewURL: URL?
    @State private var selectedTab = 0
    @State private var uploadCategory = "pictures"

    var body: some View {
        let detail = model.selectedProject!
        return NavigationStack {
            List {
                Picker("案件詳細", selection: $selectedTab) {
                    Text("案件情報").tag(0)
                    Text("書類").tag(1)
                    Text("写真").tag(2)
                }
                .pickerStyle(.segmented)
                .listRowInsets(EdgeInsets(top: 8, leading: 16, bottom: 8, trailing: 16))

                if selectedTab == 0 {
                    Section {
                        Text("\(detail.project.projectNumber)  \(detail.project.name)").font(.title3.bold())
                        Text("住所: \(detail.project.address)")
                        Text("担当者: \(detail.project.assignee ?? "未登録")")
                        if let phone = detail.project.assigneePhone, !phone.isEmpty {
                            Link("電話: \(phone)", destination: URL(string: "tel:\(phone.filter { $0.isNumber || $0 == "+" })")!)
                        } else {
                            Text("電話: 未登録")
                        }
                    }
                    if !model.offlineMode {
                        Section("この案件のオフライン保存") {
                            TextField("保存期間（日）", text: $retentionDays).keyboardType(.numberPad)
                            Button("保存期間を設定") { model.setRetentionDays(retentionDays) }
                            Button("この案件の選択ファイルを保存") {
                                let files = detail.documents.filter { selectedFileIDs.contains($0.id) } + detail.pictures.filter { selectedFileIDs.contains($0.id) }
                                model.cacheSelectedFiles(files)
                            }
                        }
                    } else {
                        Section { Text("オフライン表示中（読み取り専用）").foregroundStyle(.orange) }
                    }
                } else if selectedTab == 1 {
                    Section("書類") {
                        if !model.offlineMode {
                            Button { uploadCategory = "documents"; pickerPresented = true } label: { Label("書類をアップロード", systemImage: "arrow.up.doc") }
                        }
                        FileRows(files: detail.documents, selectable: !model.offlineMode, selected: $selectedFileIDs, open: { file in Task { previewURL = await model.previewURL(for: file) } })
                    }
                } else {
                    Section("写真") {
                        if !model.offlineMode {
                            Button { uploadCategory = "pictures"; pickerPresented = true } label: { Label("写真・動画をアップロード", systemImage: "plus.circle") }
                        }
                        PictureGrid(files: detail.pictures, selectable: !model.offlineMode, selected: $selectedFileIDs, open: { file in Task { previewURL = await model.previewURL(for: file) } })
                    }
                }
            }
            .navigationTitle(model.offlineMode ? "案件詳細（オフライン）" : "案件詳細")
            .toolbar { ToolbarItem(placement: .topBarLeading) { Button("戻る") { model.selectedProject = nil } } }
            .fileImporter(isPresented: $pickerPresented, allowedContentTypes: [.image, .movie, .pdf, .item], allowsMultipleSelection: false) { result in
                if case .success(let urls) = result, let url = urls.first { model.upload(url, category: uploadCategory) }
            }
            .sheet(isPresented: Binding(get: { previewURL != nil }, set: { if !$0 { previewURL = nil } })) {
                if let url = previewURL { QuickLookPreview(url: url) }
            }
            .onAppear { retentionDays = String(model.retentionDays); selectedTab = 0 }
        }
    }
}

struct FileRows: View {
    let files: [FileItem]
    let selectable: Bool
    @Binding var selected: Set<Int64>
    let open: (FileItem) -> Void

    var body: some View {
        if files.isEmpty {
            Text("登録されているファイルはありません").foregroundStyle(.secondary)
        } else {
            ForEach(files) { file in
                HStack(spacing: 10) {
                    if file.isImage {
                        Button { open(file) } label: {
                            FileThumbnail(file: file).frame(width: 76, height: 76).background(Color(.systemGray6)).clipShape(RoundedRectangle(cornerRadius: 8))
                        }.buttonStyle(.plain)
                    }
                    if selectable {
                        Toggle(file.filePath, isOn: Binding(get: { selected.contains(file.id) }, set: { if $0 { selected.insert(file.id) } else { selected.remove(file.id) } }))
                    } else {
                        Label(file.filePath, systemImage: file.isVideo ? "video" : "doc")
                        Spacer()
                        Button("開く") { open(file) }
                    }
                }
            }
        }
    }
}

struct PictureGrid: View {
    @EnvironmentObject private var model: AppModel
    let files: [FileItem]
    let selectable: Bool
    @Binding var selected: Set<Int64>
    let open: (FileItem) -> Void
    @State private var editingFile: FileItem?
    @State private var tagInput = ""
    private let columns = [GridItem(.flexible()), GridItem(.flexible())]

    var body: some View {
        if files.isEmpty {
            Text("登録されている写真はありません").foregroundStyle(.secondary)
        } else {
            LazyVGrid(columns: columns, spacing: 12) {
                ForEach(files) { file in
                    VStack(alignment: .leading, spacing: 6) {
                        Button { open(file) } label: {
                            FileThumbnail(file: file)
                                .frame(height: 120)
                                .frame(maxWidth: .infinity)
                                .background(Color(.systemGray6))
                                .clipShape(RoundedRectangle(cornerRadius: 8))
                        }
                        .buttonStyle(.plain)
                        Text(file.filePath).font(.caption).lineLimit(2)
                        Text(file.tag?.isEmpty == false ? "タグ: \(file.tag!)" : "タグ未設定")
                            .font(.caption)
                            .foregroundStyle(file.tag?.isEmpty == false ? Color.accentColor : Color.secondary)
                        if selectable {
                            Toggle("保存", isOn: Binding(get: { selected.contains(file.id) }, set: { if $0 { selected.insert(file.id) } else { selected.remove(file.id) } }))
                                .font(.caption)
                            Button("タグを編集") { editingFile = file; tagInput = file.tag ?? "" }
                                .font(.caption)
                        } else {
                            Button("開く") { open(file) }.font(.caption)
                        }
                    }
                    .padding(8)
                    .background(Color(.secondarySystemBackground))
                    .clipShape(RoundedRectangle(cornerRadius: 10))
                }
            }
            .alert("写真タグ", isPresented: Binding(get: { editingFile != nil }, set: { if !$0 { editingFile = nil } })) {
                TextField("タグ", text: $tagInput)
                Button("キャンセル", role: .cancel) { editingFile = nil }
                Button("保存") {
                    if let file = editingFile { model.updateTag(fileID: file.id, tag: tagInput) }
                    editingFile = nil
                }
            } message: {
                Text("空欄でタグを削除できます。")
            }
        }
    }
}

struct FileThumbnail: View {
    @EnvironmentObject private var model: AppModel
    let file: FileItem
    @State private var image: UIImage?

    var body: some View {
        Group {
            if let image {
                Image(uiImage: image).resizable().scaledToFill()
            } else if file.isVideo {
                Image(systemName: "video").font(.title).foregroundStyle(.secondary)
            } else {
                ProgressView()
            }
        }
        .task(id: file.id) { image = await model.thumbnail(for: file) }
    }
}

struct QuickLookPreview: UIViewControllerRepresentable { let url: URL; func makeUIViewController(context: Context) -> QLPreviewController { let controller = QLPreviewController(); controller.dataSource = context.coordinator; return controller }; func updateUIViewController(_ controller: QLPreviewController, context: Context) {}; func makeCoordinator() -> Coordinator { Coordinator(url: url) }; final class Coordinator: NSObject, QLPreviewControllerDataSource { let url: URL; init(url: URL) { self.url = url }; func numberOfPreviewItems(in controller: QLPreviewController) -> Int { 1 }; func previewController(_ controller: QLPreviewController, previewItemAt index: Int) -> QLPreviewItem { url as NSURL } } }

struct Project: Codable, Identifiable { let id: Int64; let projectNumber: String; let name: String; let address: String; let assignee: String?; let assigneePhone: String?; let updatedAt: String; enum CodingKeys: String, CodingKey { case id, name, address, assignee; case projectNumber = "project_number"; case assigneePhone = "assignee_phone"; case updatedAt = "updated_at" } }
struct UserMe: Codable { let id: Int64; let username: String; let role: String; let sessionExpiresAt: String; enum CodingKeys: String, CodingKey { case id, username, role; case sessionExpiresAt = "session_expires_at" } }
struct FileItem: Codable, Identifiable { let id: Int64; let filePath: String; let tag: String?; var isVideo: Bool { ["mp4", "m4v", "mov", "webm", "ogv"].contains(filePath.split(separator: ".").last?.lowercased() ?? "") }; var isImage: Bool { ["jpg", "jpeg", "png", "webp", "gif", "bmp", "heic", "heif", "avif"].contains(filePath.split(separator: ".").last?.lowercased() ?? "") }; enum CodingKeys: String, CodingKey { case id; case filePath = "file_path"; case tag } }
struct ProjectDetail: Codable { let project: Project; let documents: [FileItem]; let pictures: [FileItem] }

@MainActor
final class APIClient {
    var baseURL = "https://mbpm5.local:3443"; private let session: URLSession
    init() { let config = URLSessionConfiguration.default; config.httpCookieStorage = HTTPCookieStorage.shared; session = URLSession(configuration: config) }
    func login(username: String, password: String) async throws -> UserMe { var request = URLRequest(url: url("/login")); request.httpMethod = "POST"; request.httpBody = try JSONSerialization.data(withJSONObject: ["username": username, "password": password]); request.setValue("application/json", forHTTPHeaderField: "Content-Type"); _ = try await send(request); let (data, _) = try await session.data(for: URLRequest(url: url("/api/me"))); return try decode(data) }
    func search(number: String, name: String, address: String, sort: String) async throws -> [Project] { var c = URLComponents(string: baseURL + "/api/projects")!; c.queryItems = [URLQueryItem(name: "project_number", value: number), URLQueryItem(name: "project_name", value: name), URLQueryItem(name: "address", value: address), URLQueryItem(name: "sort", value: sort)]; let (data, _) = try await session.data(for: URLRequest(url: c.url!)); return try decode(data) }
    func detail(id: Int64) async throws -> ProjectDetail { let (data, _) = try await session.data(for: URLRequest(url: url("/api/projects/\(id)"))); return try decode(data) }
    func updateTag(fileID: Int64, tag: String) async throws -> FileItem { var request = URLRequest(url: url("/api/files/\(fileID)/tag")); request.httpMethod = "PUT"; request.httpBody = try JSONSerialization.data(withJSONObject: ["tag": tag.trimmingCharacters(in: .whitespacesAndNewlines)]); request.setValue("application/json", forHTTPHeaderField: "Content-Type"); return try decode(try await send(request)) }
    func download(fileID: Int64) async throws -> Data { let (data, response) = try await session.data(for: URLRequest(url: url("/api/files/\(fileID)/download"))); guard let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode) else { throw APIError.server }; return data }
    func upload(projectID: Int64, fileURL: URL) async throws { let access = fileURL.startAccessingSecurityScopedResource(); defer { if access { fileURL.stopAccessingSecurityScopedResource() } }; let data = try Data(contentsOf: fileURL); let boundary = "Boundary-\(UUID().uuidString)"; var request = URLRequest(url: url("/api/projects/\(projectID)/files")); request.httpMethod = "POST"; request.setValue("multipart/form-data; boundary=\(boundary)", forHTTPHeaderField: "Content-Type"); request.httpBody = Data("--\(boundary)\r\nContent-Disposition: form-data; name=\"file\"; filename=\"\(fileURL.lastPathComponent.replacingOccurrences(of: "\"", with: "_"))\"\r\nContent-Type: application/octet-stream\r\n\r\n".utf8) + data + Data("\r\n--\(boundary)--\r\n".utf8); _ = try await send(request) }
    func upload(projectID: Int64, fileURL: URL, category: String) async throws {
        let access = fileURL.startAccessingSecurityScopedResource()
        defer { if access { fileURL.stopAccessingSecurityScopedResource() } }
        let data = try Data(contentsOf: fileURL)
        let boundary = "Boundary-\(UUID().uuidString)"
        var components = URLComponents(url: url("/api/projects/\(projectID)/files"), resolvingAgainstBaseURL: false)!
        components.queryItems = [URLQueryItem(name: "category", value: category)]
        var request = URLRequest(url: components.url!)
        request.httpMethod = "POST"
        request.setValue("multipart/form-data; boundary=\(boundary)", forHTTPHeaderField: "Content-Type")
        let filename = fileURL.lastPathComponent.replacingOccurrences(of: "\"", with: "_")
        request.httpBody = Data("--\(boundary)\r\nContent-Disposition: form-data; name=\"file\"; filename=\"\(filename)\"\r\nContent-Type: application/octet-stream\r\n\r\n".utf8) + data + Data("\r\n--\(boundary)--\r\n".utf8)
        _ = try await send(request)
    }
    func logout() { var request = URLRequest(url: url("/logout")); request.httpMethod = "GET"; session.dataTask(with: request).resume(); HTTPCookieStorage.shared.cookies?.forEach { HTTPCookieStorage.shared.deleteCookie($0) } }
    private func url(_ path: String) -> URL { URL(string: baseURL + path)! }
    private func send(_ request: URLRequest) async throws -> Data { let (data, response) = try await session.data(for: request); guard let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode) else { throw APIError.server }; return data }
    private func decode<T: Decodable>(_ data: Data) throws -> T { try JSONDecoder().decode(T.self, from: data) }
}
enum APIError: LocalizedError { case server; var errorDescription: String? { "サーバーとの通信に失敗しました" } }
