package jp.filemanager.android;

import android.content.ContentResolver;
import android.net.Uri;
import org.json.JSONArray;
import org.json.JSONObject;
import java.io.*;
import java.net.*;
import java.nio.charset.StandardCharsets;
import java.util.*;

final class ApiClient {
    private final CookieManager cookies = new CookieManager(null, CookiePolicy.ACCEPT_ALL);
    private String baseUrl;
    ApiClient(String baseUrl) { this.baseUrl = trimBase(baseUrl); CookieHandler.setDefault(cookies); }
    void setBaseUrl(String value) { baseUrl = trimBase(value); }
    String getBaseUrl() { return baseUrl; }
    private static String trimBase(String value) { return value.trim().replaceAll("/+$", ""); }

    JSONObject login(String username, String password) throws Exception {
        request("POST", "/login", new JSONObject().put("username", username).put("password", password).toString(), null);
        try {
            return new JSONObject(request("GET", "/api/me", null, null));
        } catch (Exception error) {
            if (error.getMessage() != null && error.getMessage().startsWith("HTTP 401")) throw new IOException("ログイン成功後のセッションを保持できません。HTTPS接続とCookie設定を確認してください。", error);
            throw error;
        }
    }
    JSONArray search(String query, String sort) throws Exception {
        String q = "?search=" + enc(query) + "&sort=" + enc(sort);
        return new JSONArray(request("GET", "/api/projects" + q, null, null));
    }
    JSONObject updateInfo() throws Exception { return new JSONObject(request("GET", "/api/android/latest", null, null)); }
    JSONObject detail(long id) throws Exception { return new JSONObject(request("GET", "/api/projects/" + id, null, null)); }
    JSONObject updateTag(long fileId, String tag) throws Exception {
        String value = tag == null ? "" : tag.trim();
        return new JSONObject(request("PUT", "/api/files/" + fileId + "/tag", new JSONObject().put("tag", value).toString(), "application/json"));
    }
    byte[] downloadBytes(long fileId) throws Exception {
        return downloadBytes("/api/files/" + fileId + "/download");
    }
    byte[] downloadBytes(String path) throws Exception {
        HttpURLConnection c = connection(path, "GET");
        int code = c.getResponseCode();
        captureCookies(c);
        if (code < 200 || code >= 300) throw new IOException("HTTP " + code);
        try (InputStream in = c.getInputStream(); ByteArrayOutputStream out = new ByteArrayOutputStream()) {
            byte[] buffer = new byte[8192]; int n;
            while ((n = in.read(buffer)) >= 0) out.write(buffer, 0, n);
            return out.toByteArray();
        }
    }
    JSONObject upload(long projectId, ContentResolver resolver, Uri uri, String filename, String mime, String category) throws Exception {
        String boundary = "----FileManager3Android" + UUID.randomUUID();
        String suffix = category == null || category.isEmpty() ? "" : "?category=" + enc(category);
        HttpURLConnection c = connection("/api/projects/" + projectId + "/files" + suffix, "POST");
        c.setRequestProperty("Content-Type", "multipart/form-data; boundary=" + boundary);
        c.setDoOutput(true);
        try (OutputStream out = c.getOutputStream(); InputStream in = resolver.openInputStream(uri)) {
            if (in == null) throw new IOException("ファイルを開けません");
            write(out, "--" + boundary + "\r\n");
            write(out, "Content-Disposition: form-data; name=\"file\"; filename=\"" + filename.replace("\"", "_") + "\"\r\n");
            write(out, "Content-Type: " + (mime == null ? "application/octet-stream" : mime) + "\r\n\r\n");
            byte[] buffer = new byte[8192]; int n;
            while ((n = in.read(buffer)) >= 0) out.write(buffer, 0, n);
            write(out, "\r\n--" + boundary + "--\r\n");
        }
        return new JSONObject(readResponse(c));
    }
    private static void write(OutputStream out, String value) throws IOException { out.write(value.getBytes(StandardCharsets.UTF_8)); }
    private HttpURLConnection connection(String path, String method) throws Exception {
        URL url = new URL(baseUrl + path);
        HttpURLConnection c = (HttpURLConnection) url.openConnection();
        c.setRequestMethod(method); c.setConnectTimeout(10000); c.setReadTimeout(60000); c.setUseCaches(false);
        c.setRequestProperty("Accept", "application/json"); String cookie = cookieHeader(); if (!cookie.isEmpty()) c.setRequestProperty("Cookie", cookie); return c;
    }
    private String request(String method, String path, String body, String contentType) throws Exception {
        HttpURLConnection c = connection(path, method);
        if (body != null) { c.setDoOutput(true); c.setRequestProperty("Content-Type", contentType == null ? "application/json" : contentType); try (OutputStream out = c.getOutputStream()) { write(out, body); } }
        return readResponse(c);
    }
    private String readResponse(HttpURLConnection c) throws Exception {
        int code = c.getResponseCode(); captureCookies(c); InputStream stream = code >= 400 ? c.getErrorStream() : c.getInputStream();
        String text = stream == null ? "" : read(stream);
        if (code < 200 || code >= 300) throw new IOException("HTTP " + code + (text.isEmpty() ? "" : ": " + text));
        return text;
    }
    private String cookieHeader() { StringBuilder value = new StringBuilder(); for (HttpCookie cookie : cookies.getCookieStore().getCookies()) { if (value.length() > 0) value.append("; "); value.append(cookie.getName()).append('=').append(cookie.getValue()); } return value.toString(); }
    private void captureCookies(HttpURLConnection c) { try { cookies.put(c.getURL().toURI(), c.getHeaderFields()); } catch (Exception ignored) {} }
    private static String read(InputStream in) throws IOException { StringBuilder b = new StringBuilder(); try (BufferedReader r = new BufferedReader(new InputStreamReader(in, StandardCharsets.UTF_8))) { String line; while ((line = r.readLine()) != null) b.append(line); } return b.toString(); }
    private static String enc(String value) throws UnsupportedEncodingException { return URLEncoder.encode(value == null ? "" : value, "UTF-8"); }
}
