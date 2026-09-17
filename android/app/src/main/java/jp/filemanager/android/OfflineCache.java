package jp.filemanager.android;

import android.content.Context;
import android.content.SharedPreferences;
import org.json.JSONArray;
import org.json.JSONObject;
import java.io.*;
import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.util.*;

final class OfflineCache {
    private static final String PREFIX = "project_";
    private final SharedPreferences preferences;
    private final File root;

    OfflineCache(Context context) {
        preferences = context.getSharedPreferences("filemanager_offline", Context.MODE_PRIVATE);
        root = new File(context.getFilesDir(), "offline-projects");
    }

    int retentionDays() { int value = preferences.getInt("retention_days", 30); return Math.max(1, Math.min(3650, value)); }
    void setRetentionDays(int days) { preferences.edit().putInt("retention_days", Math.max(1, Math.min(3650, days))).apply(); }

    void cleanupExpired() {
        long now = System.currentTimeMillis(); SharedPreferences.Editor editor = preferences.edit();
        for (Map.Entry<String, ?> entry : preferences.getAll().entrySet()) {
            if (!entry.getKey().startsWith(PREFIX) || !(entry.getValue() instanceof String)) continue;
            try { JSONObject detail = new JSONObject((String) entry.getValue()); if (detail.optLong("offline_expires_at", 0) <= now) { deleteProjectFiles(detail.optString("offline_server_key"), detail.optLong("id")); editor.remove(entry.getKey()); } }
            catch (Exception ignored) { editor.remove(entry.getKey()); }
        }
        editor.apply();
    }

    void saveProject(String serverUrl, JSONObject original, Map<Long, byte[]> fileBytes) throws Exception {
        JSONObject detail = new JSONObject(original.toString()); JSONObject project = detail.optJSONObject("project"); long projectId = project == null ? 0 : project.optLong("id");
        String serverKey = key(serverUrl); long now = System.currentTimeMillis(); long expires = now + retentionDays() * 24L * 60L * 60L * 1000L;
        Set<Long> idSet = new LinkedHashSet<>(); String cacheKey = projectKey(serverKey, projectId); String previous = preferences.getString(cacheKey, null);
        if (previous != null) { JSONArray previousIds = new JSONObject(previous).optJSONArray("offline_file_ids"); if (previousIds != null) for (int i = 0; i < previousIds.length(); i++) idSet.add(previousIds.optLong(i)); }
        File directory = new File(new File(root, serverKey), String.valueOf(projectId));
        if (!directory.exists() && !directory.mkdirs()) throw new IOException("オフライン保存先を作成できません。");
        for (Map.Entry<Long, byte[]> entry : fileBytes.entrySet()) { File target = new File(directory, entry.getKey() + ".bin"); File temporary = new File(directory, entry.getKey() + ".tmp"); try (FileOutputStream out = new FileOutputStream(temporary)) { out.write(entry.getValue()); } if (!temporary.renameTo(target)) throw new IOException("ファイル保存に失敗しました。"); idSet.add(entry.getKey()); }
        JSONArray ids = new JSONArray(); for (long id : idSet) ids.put(id);
        detail.put("offline_file_ids", ids); detail.put("offline_server_key", serverKey); detail.put("offline_cached_at", now); detail.put("offline_expires_at", expires);
        preferences.edit().putString(cacheKey, detail.toString()).apply();
    }

    JSONArray cachedProjects(String serverUrl) {
        cleanupExpired(); JSONArray result = new JSONArray(); String serverKey = key(serverUrl);
        for (Map.Entry<String, ?> entry : preferences.getAll().entrySet()) {
            if (!entry.getKey().startsWith(PREFIX + serverKey + "_") || !(entry.getValue() instanceof String)) continue;
            try { JSONObject detail = new JSONObject((String) entry.getValue()), project = detail.optJSONObject("project"); if (project != null) { JSONObject summary = new JSONObject(project.toString()); summary.put("offline", true); result.put(summary); } } catch (Exception ignored) { }
        }
        return result;
    }

    JSONObject loadProject(String serverUrl, long projectId) throws Exception {
        cleanupExpired(); String serverKey = key(serverUrl); String raw = preferences.getString(projectKey(serverKey, projectId), null); if (raw == null) return null;
        JSONObject detail = new JSONObject(raw); JSONArray ids = detail.optJSONArray("offline_file_ids"); Set<String> available = new HashSet<>();
        if (ids != null) for (int i = 0; i < ids.length(); i++) if (file(serverKey, projectId, ids.optLong(i)).isFile()) available.add(String.valueOf(ids.optLong(i)));
        filterFiles(detail, "documents", available); filterFiles(detail, "pictures", available); return detail;
    }

    File fileFor(String serverUrl, long projectId, long fileId) { return file(key(serverUrl), projectId, fileId); }
    void updateExpiration(String serverUrl, long projectId) throws Exception { String serverKey = key(serverUrl), cacheKey = projectKey(serverKey, projectId), raw = preferences.getString(cacheKey, null); if (raw == null) return; JSONObject detail = new JSONObject(raw); detail.put("offline_expires_at", System.currentTimeMillis() + retentionDays() * 24L * 60L * 60L * 1000L); preferences.edit().putString(cacheKey, detail.toString()).apply(); }
    void clearAll() { deleteRecursively(root); preferences.edit().clear().apply(); }

    private void filterFiles(JSONObject detail, String field, Set<String> available) throws Exception { JSONArray source = detail.optJSONArray(field), filtered = new JSONArray(); if (source != null) for (int i = 0; i < source.length(); i++) { JSONObject item = source.optJSONObject(i); if (item != null && available.contains(String.valueOf(item.optLong("id")))) filtered.put(item); } detail.put(field, filtered); }
    private String projectKey(String serverKey, long projectId) { return PREFIX + serverKey + "_" + projectId; }
    private File file(String serverKey, long projectId, long fileId) { return new File(new File(new File(root, serverKey), String.valueOf(projectId)), fileId + ".bin"); }
    private void deleteProjectFiles(String serverKey, long projectId) { if (serverKey != null && !serverKey.isEmpty()) deleteRecursively(new File(new File(root, serverKey), String.valueOf(projectId))); }
    private static void deleteRecursively(File file) { if (!file.exists()) return; if (file.isDirectory()) { File[] children = file.listFiles(); if (children != null) for (File child : children) deleteRecursively(child); } file.delete(); }
    private static String key(String value) { try { byte[] digest = MessageDigest.getInstance("SHA-256").digest(value.trim().getBytes(StandardCharsets.UTF_8)); StringBuilder out = new StringBuilder(); for (byte b : digest) out.append(String.format(Locale.ROOT, "%02x", b)); return out.substring(0, 24); } catch (Exception e) { throw new IllegalStateException(e); } }
}
