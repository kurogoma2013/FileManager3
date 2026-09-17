package jp.filemanager.android;

import android.app.*;
import android.content.*;
import android.graphics.Color;
import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
import android.graphics.drawable.GradientDrawable;
import android.net.Uri;
import android.os.Bundle;
import android.os.Build;
import android.provider.Settings;
import android.text.InputType;
import android.view.Gravity;
import android.view.View;
import android.webkit.MimeTypeMap;
import android.widget.*;
import androidx.core.content.FileProvider;
import org.json.*;
import java.io.*;
import java.util.*;
import java.util.concurrent.*;

public class MainActivity extends Activity {
    private static final int PICK_FILE = 42;
    private static final String DEFAULT_BASE_URL = "https://goma2013.com";
    private final ExecutorService executor = Executors.newSingleThreadExecutor();
    private final ApiClient api = new ApiClient(DEFAULT_BASE_URL);
    private OfflineCache offlineCache;
    private LinearLayout content;
    private TextView message;
    private long currentProjectId;
    private JSONObject currentDetail;
    private String currentUserRole = "";
    private String pendingUploadCategory = "pictures";
    private boolean offlineMode;
    private boolean updateCheckStarted;
    private final Map<Long, CheckBox> selectedFiles = new LinkedHashMap<>();

    @Override public void onCreate(Bundle state) { super.onCreate(state); offlineCache = new OfflineCache(this); offlineCache.cleanupExpired(); showLogin(); checkForUpdate(); }
    @Override protected void onDestroy() { executor.shutdownNow(); super.onDestroy(); }
    private TextView text(String value, float size) { TextView v = new TextView(this); v.setText(value); v.setTextSize(size); v.setTextColor(Color.rgb(15,23,42)); v.setPadding(0, dp(8), 0, dp(8)); return v; }
    private int dp(float value) { return Math.round(value * getResources().getDisplayMetrics().density); }
    private GradientDrawable rounded(int fill, int stroke, float radius) { GradientDrawable drawable = new GradientDrawable(); drawable.setColor(fill); drawable.setCornerRadius(dp(radius)); if (stroke != 0) drawable.setStroke(dp(1), stroke); return drawable; }
    private EditText input(String hint) { EditText e = new EditText(this); e.setHint(hint); e.setSingleLine(true); e.setTextSize(16); e.setPadding(dp(12), 0, dp(12), 0); e.setMinHeight(dp(44)); e.setBackground(rounded(Color.rgb(251,252,254), Color.rgb(219,226,236), 8)); return e; }
    private Button button(String label) { Button b = new Button(this); b.setText(label); b.setAllCaps(false); b.setTextSize(14); b.setMinHeight(dp(44)); b.setPadding(dp(12), 0, dp(12), 0); b.setBackground(rounded(Color.rgb(57,119,232), 0, 8)); b.setTextColor(Color.WHITE); return b; }
    private TextView fieldLabel(String value) { TextView v = text(value, 12); v.setTextColor(Color.rgb(89,103,123)); v.setTypeface(null, 1); v.setPadding(0, dp(16), 0, dp(6)); return v; }
    private TextView versionLabel() { TextView v = text(appVersionLabel(), 10); v.setTextColor(Color.rgb(100,116,139)); v.setPadding(0, dp(3), 0, 0); return v; }
    private String appVersionLabel() { String version = BuildConfig.VERSION_NAME; return version.startsWith("0.0.") && version.length() > 12 ? "v" + version.substring(4, 12) + "." + version.substring(12) : "v" + version; }
    private LinearLayout column() { LinearLayout l = new LinearLayout(this); l.setOrientation(LinearLayout.VERTICAL); l.setPadding(dp(28), dp(24), dp(28), dp(24)); return l; }
    private LinearLayout panel() { LinearLayout l = column(); l.setBackground(rounded(Color.WHITE, Color.rgb(226,232,240), 8)); l.setElevation(dp(2)); return l; }
    private Button secondaryButton(String label) { Button b = button(label); b.setTextColor(Color.rgb(71,85,105)); b.setBackground(rounded(Color.WHITE, Color.rgb(226,232,240), 8)); return b; }
    private LinearLayout labeledRow(String label, String value) { LinearLayout row = new LinearLayout(this); row.setOrientation(LinearLayout.HORIZONTAL); row.setGravity(Gravity.TOP); row.setPadding(0, dp(4), 0, dp(4)); TextView key = text(label, 11); key.setTextColor(Color.rgb(100,116,139)); key.setTypeface(null, 1); row.addView(key, new LinearLayout.LayoutParams(dp(72), -2)); TextView val = text(value == null || value.isEmpty() ? "未登録" : value, 13); val.setLayoutParams(new LinearLayout.LayoutParams(0, -2, 1)); row.addView(val); return row; }
    private void baseScreen(String title) { LinearLayout root = new LinearLayout(this); root.setOrientation(LinearLayout.VERTICAL); root.setBackgroundColor(Color.rgb(248,250,252)); LinearLayout topbar = new LinearLayout(this); topbar.setGravity(Gravity.CENTER_VERTICAL); topbar.setPadding(dp(16), 0, dp(12), 0); topbar.setMinimumHeight(dp(62)); topbar.setBackgroundColor(Color.WHITE); TextView mark = text("⌂", 18); mark.setTextColor(Color.rgb(57,119,232)); mark.setGravity(Gravity.CENTER); topbar.addView(mark, new LinearLayout.LayoutParams(dp(32), dp(44))); TextView breadcrumb = text(title, 13); breadcrumb.setTypeface(null, 1); breadcrumb.setTextColor(Color.rgb(100,116,139)); topbar.addView(breadcrumb, new LinearLayout.LayoutParams(0, -2, 1)); if (canManage()) { Button management = secondaryButton("管理メニュー"); topbar.addView(management, new LinearLayout.LayoutParams(dp(112), dp(44))); management.setOnClickListener(v -> showManagementMenu()); } Button logout = secondaryButton("ログアウト"); topbar.addView(logout, new LinearLayout.LayoutParams(dp(88), dp(44))); logout.setOnClickListener(v -> showLogin()); root.addView(topbar); ScrollView scroll = new ScrollView(this); scroll.setFillViewport(true); content = column(); content.setPadding(dp(16), dp(24), dp(16), dp(24)); scroll.addView(content, new ScrollView.LayoutParams(-1, -2)); root.addView(scroll, new LinearLayout.LayoutParams(-1, 0, 1)); content.addView(text(title, 24)); message = text("", 14); message.setTextColor(Color.rgb(180, 35, 35)); content.addView(message); setContentView(root); }
    private boolean canManage() { return "admin".equals(currentUserRole) || "member".equals(currentUserRole); }
    private void info(String value) { if (message != null) message.setText(value == null ? "" : value); }
    private interface Task { void run() throws Exception; }
    private void runAsync(Task work) { executor.execute(() -> { try { work.run(); } catch (Exception e) { runOnUiThread(() -> info(errorMessage(e))); } }); }
    private String errorMessage(Exception error) { String value = error.getMessage(); if (value != null && value.startsWith("HTTP 401")) return "ユーザー名またはパスワードが違うか、ログインが一時的に制限されています。15分後に再試行してください。"; return value == null || value.isEmpty() ? "通信に失敗しました" : value; }
    private void checkForUpdate() { if (updateCheckStarted) return; updateCheckStarted = true; executor.execute(() -> { try { JSONObject update = api.updateInfo(); if (update.optLong("version_code", 0) > BuildConfig.VERSION_CODE) runOnUiThread(() -> showUpdateDialog(update)); } catch (Exception ignored) { } }); }
    private void showUpdateDialog(JSONObject update) { if (isFinishing()) return; String version = update.optString("version_name", "新しいバージョン"); String notes = update.optString("release_notes", ""); String message = version + "が利用できます。" + (notes.isEmpty() ? "" : "\n" + notes) + "\n更新を開始しますか？"; new AlertDialog.Builder(this).setTitle("アップデートがあります").setMessage(message).setNegativeButton("後で", null).setPositiveButton("更新", (dialog, which) -> downloadAndInstallUpdate(update)).show(); }
    private void downloadAndInstallUpdate(JSONObject update) { runAsync(() -> { String path = update.optString("download_url", ""); if (!path.startsWith("/android/")) throw new IOException("更新先が不正です。"); byte[] apk = api.downloadBytes(path); String expected = update.optString("sha256", ""); if (expected.isEmpty() || !expected.equalsIgnoreCase(sha256(apk))) throw new IOException("ダウンロードしたAPKの検証に失敗しました。"); File directory = new File(getCacheDir(), "updates"); if (!directory.exists() && !directory.mkdirs()) throw new IOException("更新ファイルの保存先を作成できません。"); File temporary = new File(directory, "filemanager3-update.tmp"); File target = new File(directory, "filemanager3-update.apk"); try (FileOutputStream out = new FileOutputStream(temporary)) { out.write(apk); } if (!temporary.renameTo(target)) throw new IOException("更新ファイルの保存に失敗しました。"); runOnUiThread(() -> installApk(target)); }); }
    private String sha256(byte[] value) throws Exception { byte[] digest = java.security.MessageDigest.getInstance("SHA-256").digest(value); StringBuilder result = new StringBuilder(); for (byte item : digest) result.append(String.format(Locale.ROOT, "%02x", item)); return result.toString(); }
    private void installApk(File apk) { if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O && !getPackageManager().canRequestPackageInstalls()) { info("更新には「不明なアプリのインストール」を許可してください。"); startActivity(new Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:" + getPackageName()))); return; } Uri uri = FileProvider.getUriForFile(this, getPackageName() + ".fileprovider", apk); Intent intent = new Intent(Intent.ACTION_INSTALL_PACKAGE); intent.setDataAndType(uri, "application/vnd.android.package-archive"); intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION); startActivity(intent); }

    private void showLogin() {
        offlineMode = false; api.setBaseUrl(DEFAULT_BASE_URL);
        FrameLayout root = new FrameLayout(this); root.setBackgroundColor(Color.rgb(246,248,251));
        ScrollView scroll = new ScrollView(this); FrameLayout.LayoutParams scrollParams = new FrameLayout.LayoutParams(-1, -1); root.addView(scroll, scrollParams);
        scroll.setFillViewport(true); LinearLayout page = new LinearLayout(this); page.setOrientation(LinearLayout.VERTICAL); page.setGravity(Gravity.CENTER); page.setPadding(dp(16), dp(24), dp(16), dp(24)); scroll.addView(page, new ScrollView.LayoutParams(-1, -1));
        LinearLayout panel = new LinearLayout(this); panel.setOrientation(LinearLayout.VERTICAL); panel.setPadding(dp(32), dp(32), dp(32), dp(32)); panel.setBackground(rounded(Color.WHITE, Color.rgb(226,232,240), 8)); panel.setElevation(dp(8)); LinearLayout.LayoutParams panelParams = new LinearLayout.LayoutParams(Math.min(dp(420), getResources().getDisplayMetrics().widthPixels - dp(32)), -2); panelParams.gravity = Gravity.CENTER; page.addView(panel, panelParams);
        LinearLayout brand = new LinearLayout(this); brand.setOrientation(LinearLayout.HORIZONTAL); brand.setGravity(Gravity.CENTER_VERTICAL); TextView mark = text("⌂", 20); mark.setGravity(Gravity.CENTER); mark.setTextColor(Color.WHITE); mark.setBackground(rounded(Color.rgb(57,119,232), 0, 8)); brand.addView(mark, new LinearLayout.LayoutParams(dp(38), dp(38))); LinearLayout brandText = new LinearLayout(this); brandText.setOrientation(LinearLayout.VERTICAL); TextView name = text("FileManager3", 18); name.setTypeface(null, 1); brandText.addView(name); brandText.addView(versionLabel()); LinearLayout.LayoutParams brandTextParams = new LinearLayout.LayoutParams(-2, -2); brandTextParams.setMargins(dp(12), 0, 0, 0); brand.addView(brandText, brandTextParams); LinearLayout.LayoutParams brandParams = new LinearLayout.LayoutParams(-1, -2); brandParams.setMargins(0, 0, 0, dp(28)); panel.addView(brand, brandParams);
        TextView title = text("ログイン", 22); title.setTypeface(null, 1); title.setPadding(0, 0, 0, dp(6)); panel.addView(title); TextView lead = text("ユーザー名とパスワードを入力してください。", 13); lead.setTextColor(Color.rgb(100,116,139)); lead.setPadding(0, 0, 0, dp(8)); panel.addView(lead);
        EditText user = input("ユーザー名"); EditText pass = input("パスワード"); pass.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_PASSWORD); TextView passToggle = text("表示", 12); passToggle.setTextColor(Color.rgb(57,119,232)); passToggle.setGravity(Gravity.CENTER); passToggle.setOnClickListener(v -> { boolean visible = pass.getInputType() == (InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD); pass.setInputType(visible ? (InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_PASSWORD) : (InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD)); pass.setSelection(pass.length()); passToggle.setText(visible ? "表示" : "非表示"); });
        panel.addView(fieldLabel("ユーザー名")); panel.addView(user); panel.addView(fieldLabel("パスワード")); FrameLayout passwordFrame = new FrameLayout(this); passwordFrame.addView(pass, new FrameLayout.LayoutParams(-1, dp(44))); FrameLayout.LayoutParams toggleParams = new FrameLayout.LayoutParams(dp(56), dp(44), Gravity.END); passwordFrame.addView(passToggle, toggleParams); panel.addView(passwordFrame);
        Button login = button("ログイン"); LinearLayout.LayoutParams loginParams = new LinearLayout.LayoutParams(-1, dp(44)); loginParams.setMargins(0, dp(22), 0, 0); panel.addView(login, loginParams); Button offline = button("保存済み案件をオフライン表示"); offline.setTextColor(Color.rgb(57,119,232)); offline.setBackground(rounded(Color.WHITE, Color.rgb(155,188,245), 8)); LinearLayout.LayoutParams offlineParams = new LinearLayout.LayoutParams(-1, dp(44)); offlineParams.setMargins(0, dp(10), 0, 0); panel.addView(offline, offlineParams); Button clear = button("オフラインキャッシュを削除"); clear.setTextColor(Color.rgb(185,28,28)); clear.setBackground(rounded(Color.WHITE, Color.rgb(231,235,242), 8)); LinearLayout.LayoutParams clearParams = new LinearLayout.LayoutParams(-1, dp(44)); clearParams.setMargins(0, dp(10), 0, 0); panel.addView(clear, clearParams); message = text("", 13); message.setTextColor(Color.rgb(185,28,28)); message.setPadding(0, dp(14), 0, 0); panel.addView(message);
        setContentView(root);
        login.setOnClickListener(v -> { String username = user.getText().toString().trim(); String password = pass.getText().toString(); if (username.isEmpty() || password.isEmpty()) { info("ユーザー名とパスワードを入力してください。"); return; } info(""); login.setEnabled(false); runAsync(() -> { try { JSONObject me = api.login(username, password); currentUserRole = me.optString("role", ""); runOnUiThread(this::showProjects); } catch (Exception error) { runOnUiThread(() -> { login.setEnabled(true); info(errorMessage(error)); }); } }); });
        clear.setOnClickListener(v -> { offlineCache.clearAll(); info("オフラインキャッシュを削除しました。"); });
        offline.setOnClickListener(v -> showOfflineProjects());
    }

    private void showProjects() {
        offlineMode = false; baseScreen("案件検索");
        LinearLayout searchCard = panel(); searchCard.setPadding(dp(20), dp(18), dp(20), dp(20));
        TextView hint = text("案件名・案件番号・カナ・住所・販売店で検索できます", 13); hint.setTextColor(Color.rgb(100,116,139)); searchCard.addView(hint);
        EditText query = input("案件名・案件番号・カナ・住所・販売店など"); LinearLayout.LayoutParams queryParams = new LinearLayout.LayoutParams(-1, dp(48)); queryParams.setMargins(0, dp(10), 0, 0); searchCard.addView(query, queryParams);
        Spinner sort = new Spinner(this); String[] labels = {"更新日時の新しい順", "更新日時の古い順", "案件名順（昇順）", "案件名順（降順）", "案件番号順（昇順）", "案件番号順（降順）"}; sort.setAdapter(new ArrayAdapter<>(this, android.R.layout.simple_spinner_dropdown_item, labels)); searchCard.addView(text("並び順", 11)); searchCard.addView(sort);
        Button search = button("検索する"); LinearLayout.LayoutParams searchParams = new LinearLayout.LayoutParams(-1, dp(48)); searchParams.setMargins(0, dp(12), 0, 0); searchCard.addView(search, searchParams); content.addView(searchCard);
        LinearLayout resultHeader = new LinearLayout(this); resultHeader.setGravity(Gravity.CENTER_VERTICAL); TextView resultTitle = text("検索結果", 18); resultTitle.setTypeface(null, 1); resultHeader.addView(resultTitle, new LinearLayout.LayoutParams(0, -2, 1)); TextView resultCount = text("0件", 13); resultCount.setTextColor(Color.rgb(100,116,139)); resultHeader.addView(resultCount); LinearLayout.LayoutParams headerParams = new LinearLayout.LayoutParams(-1, -2); headerParams.setMargins(0, dp(22), 0, dp(8)); content.addView(resultHeader, headerParams);
        LinearLayout results = new LinearLayout(this); results.setOrientation(LinearLayout.VERTICAL); content.addView(results);
        search.setOnClickListener(v -> { search.setEnabled(false); runAsync(() -> { try { String[] values = {"updated_desc", "updated_asc", "name_asc", "name_desc", "number_asc", "number_desc"}; JSONArray rows = api.search(query.getText().toString().trim(), values[sort.getSelectedItemPosition()]); runOnUiThread(() -> { search.setEnabled(true); resultCount.setText(rows.length() + "件"); results.removeAllViews(); renderProjects(results, rows); }); } catch (Exception error) { JSONArray rows = offlineCache.cachedProjects(api.getBaseUrl()); if (rows.length() == 0) throw error; runOnUiThread(() -> { search.setEnabled(true); resultCount.setText(rows.length() + "件"); info("オフライン表示中（保存済み案件のみ）"); results.removeAllViews(); renderProjects(results, rows); }); } }); });
        search.performClick();
    }

    private void showOfflineProjects() { offlineMode = true; baseScreen("保存済み案件（オフライン）"); Button back = secondaryButton("ログイン画面へ"); content.addView(back); back.setOnClickListener(v -> showLogin()); TextView hint = text("保存期間内の案件情報を表示しています。", 13); hint.setTextColor(Color.rgb(100,116,139)); content.addView(hint); JSONArray rows = offlineCache.cachedProjects(api.getBaseUrl()); renderProjects(content, rows); if (rows.length() == 0) content.addView(text("保存済み案件がありません。", 14)); }
    private void showManagementMenu() { baseScreen("管理メニュー"); Button back = secondaryButton("← 案件検索"); content.addView(back); back.setOnClickListener(v -> showProjects()); LinearLayout menu = panel(); menu.setPadding(dp(16), dp(14), dp(16), dp(16)); TextView hint = text("管理画面をブラウザで開きます。", 13); hint.setTextColor(Color.rgb(100,116,139)); menu.addView(hint); addManagementButton(menu, "案件管理", "/admin/projects"); addManagementButton(menu, "販売店管理", "/admin/dealers"); if ("admin".equals(currentUserRole)) { addManagementButton(menu, "ユーザー管理", "/admin/users"); addManagementButton(menu, "仕様書", "/admin/specifications"); } content.addView(menu); }
    private void addManagementButton(LinearLayout target, String label, String path) { Button open = secondaryButton(label); LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(-1, dp(44)); params.setMargins(0, dp(8), 0, 0); target.addView(open, params); open.setOnClickListener(v -> openManagementPage(path)); }
    private void openManagementPage(String path) { try { startActivity(new Intent(Intent.ACTION_VIEW, Uri.parse(api.getBaseUrl() + path))); } catch (Exception error) { info("管理画面を開けるブラウザがありません。"); } }
    private void openFilePicker(String category) { pendingUploadCategory = category; Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT); intent.setType("*/*"); intent.putExtra(Intent.EXTRA_ALLOW_MULTIPLE, false); intent.addCategory(Intent.CATEGORY_OPENABLE); startActivityForResult(intent, PICK_FILE); }
    private void renderProjects(LinearLayout results, JSONArray rows) { for (int i = 0; i < rows.length(); i++) { JSONObject p = rows.optJSONObject(i); if (p == null) continue; long id = p.optLong("id"); LinearLayout card = panel(); card.setPadding(dp(14), dp(8), dp(14), dp(8)); card.setOnClickListener(v -> loadDetail(id)); card.addView(labeledRow("案件番号", p.optString("project_number"))); LinearLayout nameRow = new LinearLayout(this); nameRow.setOrientation(LinearLayout.HORIZONTAL); nameRow.setGravity(Gravity.TOP); nameRow.setPadding(0, dp(4), 0, dp(4)); TextView nameLabel = text("案件名", 11); nameLabel.setTextColor(Color.rgb(100,116,139)); nameLabel.setTypeface(null, 1); nameRow.addView(nameLabel, new LinearLayout.LayoutParams(dp(72), -2)); LinearLayout nameValue = new LinearLayout(this); nameValue.setOrientation(LinearLayout.VERTICAL); TextView projectName = text(p.optString("name"), 15); projectName.setTypeface(null, 1); nameValue.addView(projectName); TextView kana = text(p.optString("kana", ""), 11); kana.setTextColor(Color.rgb(100,116,139)); nameValue.addView(kana); nameRow.addView(nameValue, new LinearLayout.LayoutParams(0, -2, 1)); card.addView(nameRow); card.addView(labeledRow("登録データ", "書類: " + p.optInt("documents_count", 0) + "　写真: " + p.optInt("pictures_count", 0))); String address = p.optString("address", "未登録"); String position = p.optString("plus_code", ""); if (position.isEmpty() && !p.isNull("latitude") && !p.isNull("longitude")) position = p.optString("latitude") + ", " + p.optString("longitude"); card.addView(labeledRow("住所", position.isEmpty() ? address : address + "\n" + position)); card.addView(labeledRow("販売店", p.optString("dealer", "未登録"))); card.addView(labeledRow("更新日時", p.optString("updated_at", "未登録"))); TextView openHint = text("詳細を開く  ›", 12); openHint.setTextColor(Color.rgb(57,119,232)); openHint.setGravity(Gravity.RIGHT); card.addView(openHint); LinearLayout.LayoutParams cardParams = new LinearLayout.LayoutParams(-1, -2); cardParams.setMargins(0, 0, 0, dp(8)); results.addView(card, cardParams); } }

    private void loadDetail(long id) { runAsync(() -> { try { JSONObject detail = api.detail(id); runOnUiThread(() -> showDetail(detail, false)); } catch (Exception error) { try { JSONObject cached = offlineCache.loadProject(api.getBaseUrl(), id); if (cached == null) throw error; runOnUiThread(() -> showDetail(cached, true)); } catch (Exception ignored) { throw error; } } }); }
    private void showDetail(JSONObject d, boolean cached) {
        offlineMode = cached;
        currentDetail = d;
        JSONObject p = d.optJSONObject("project");
        currentProjectId = p == null ? 0 : p.optLong("id");
        selectedFiles.clear();
        baseScreen(cached ? "案件詳細（オフライン）" : "案件詳細");
        Button back = secondaryButton("← 案件一覧");
        content.addView(back);
        back.setOnClickListener(v -> { if (cached) showOfflineProjects(); else showProjects(); });

        LinearLayout tabs = new LinearLayout(this);
        tabs.setOrientation(LinearLayout.HORIZONTAL);
        tabs.setPadding(0, dp(12), 0, dp(12));
        content.addView(tabs);
        FrameLayout tabContent = new FrameLayout(this);
        content.addView(tabContent, new LinearLayout.LayoutParams(-1, -2));
        LinearLayout infoTab = column(); infoTab.setPadding(0, 0, 0, 0); tabContent.addView(infoTab);
        LinearLayout filesTab = column(); filesTab.setPadding(0, 0, 0, 0); filesTab.setVisibility(View.GONE); tabContent.addView(filesTab);
        LinearLayout picturesTab = column(); picturesTab.setPadding(0, 0, 0, 0); picturesTab.setVisibility(View.GONE); tabContent.addView(picturesTab);
        Button infoButton = tabButton("案件情報", true);
        Button documentsButton = tabButton("書類", false);
        Button picturesButton = tabButton("写真", false);
        tabs.addView(infoButton, tabParams()); tabs.addView(documentsButton, tabParams()); tabs.addView(picturesButton, tabParams());
        infoButton.setOnClickListener(v -> selectDetailTab(infoButton, documentsButton, picturesButton, infoTab, filesTab, picturesTab));
        documentsButton.setOnClickListener(v -> selectDetailTab(documentsButton, infoButton, picturesButton, filesTab, infoTab, picturesTab));
        picturesButton.setOnClickListener(v -> selectDetailTab(picturesButton, infoButton, documentsButton, picturesTab, infoTab, filesTab));

        LinearLayout summary = panel();
        summary.setPadding(dp(16), dp(14), dp(16), dp(14));
        TextView number = text(p.optString("project_number"), 12);
        number.setTextColor(Color.rgb(39,88,184)); number.setTypeface(null, 1);
        number.setBackground(rounded(Color.rgb(238,244,255), 0, 7)); number.setPadding(dp(9), dp(4), dp(9), dp(4)); summary.addView(number);
        TextView name = text(p.optString("name"), 23); name.setTypeface(null, 1); name.setPadding(0, dp(12), 0, 0); summary.addView(name);
        TextView kana = text(p.optString("kana", ""), 11); kana.setTextColor(Color.rgb(100,116,139)); kana.setPadding(0, dp(2), 0, dp(8)); summary.addView(kana);
        summary.addView(labeledRow("住所", p.optString("address", "未登録"))); summary.addView(labeledRow("担当者", p.optString("assignee", "未登録")));
        summary.addView(labeledRow("電話", p.optString("assignee_phone", "未登録"))); summary.addView(labeledRow("更新日時", p.optString("updated_at", "未登録"))); infoTab.addView(summary);
        if (!cached) {
            LinearLayout offlinePanel = panel(); offlinePanel.setPadding(dp(16), dp(12), dp(16), dp(16));
            offlinePanel.addView(text("この案件をオフライン保存", 16));
            TextView offlineHint = text("書類・写真タブでファイルを選択し、この案件単位で保存します。", 12); offlineHint.setTextColor(Color.rgb(100,116,139)); offlinePanel.addView(offlineHint);
            Button save = button("この案件の選択ファイルを保存"); offlinePanel.addView(save); save.setOnClickListener(v -> cacheSelectedFiles()); infoTab.addView(offlinePanel);
        } else {
            TextView readOnly = text("この案件の保存済みファイルのみ閲覧できます（読み取り専用）。", 13); readOnly.setTextColor(Color.rgb(100,116,139)); infoTab.addView(readOnly);
        }
        addFileSection(filesTab, "書類", d.optJSONArray("documents"), !cached, false);
        addFileSection(picturesTab, "写真", d.optJSONArray("pictures"), !cached, true);
    }

    private Button tabButton(String label, boolean selected) { Button b = secondaryButton(label); b.setTextSize(13); b.setTextColor(selected ? Color.WHITE : Color.rgb(71,85,105)); b.setBackground(rounded(selected ? Color.rgb(57,119,232) : Color.WHITE, selected ? Color.rgb(57,119,232) : Color.rgb(226,232,240), 8)); return b; }
    private LinearLayout.LayoutParams tabParams() { LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(0, dp(44), 1); params.setMargins(dp(3), 0, dp(3), 0); return params; }
    private void selectDetailTab(Button selected, Button otherOne, Button otherTwo, View selectedView, View otherViewOne, View otherViewTwo) { selected.setTextColor(Color.WHITE); selected.setBackground(rounded(Color.rgb(57,119,232), Color.rgb(57,119,232), 8)); otherOne.setTextColor(Color.rgb(71,85,105)); otherOne.setBackground(rounded(Color.WHITE, Color.rgb(226,232,240), 8)); otherTwo.setTextColor(Color.rgb(71,85,105)); otherTwo.setBackground(rounded(Color.WHITE, Color.rgb(226,232,240), 8)); selectedView.setVisibility(View.VISIBLE); otherViewOne.setVisibility(View.GONE); otherViewTwo.setVisibility(View.GONE); }
    private void addFileSection(LinearLayout target, String title, JSONArray files, boolean selectable, boolean pictures) { LinearLayout section = panel(); section.setPadding(dp(16), dp(14), dp(16), dp(12)); LinearLayout header = new LinearLayout(this); header.setGravity(Gravity.CENTER_VERTICAL); TextView heading = text(title, 16); heading.setTypeface(null, 1); header.addView(heading, new LinearLayout.LayoutParams(0, -2, 1)); if (selectable) { Button upload = button(pictures ? "写真をアップロード" : "書類をアップロード"); upload.setTextSize(12); header.addView(upload, new LinearLayout.LayoutParams(-2, dp(44))); upload.setOnClickListener(v -> openFilePicker(pictures ? "pictures" : "documents")); } section.addView(header); if (pictures) renderPictureGrid(section, files, selectable); else renderFiles(section, files, selectable); target.addView(section); }

    private void renderFiles(LinearLayout target, JSONArray files, boolean selectable) { if (files == null || files.length() == 0) { TextView empty = text("登録されているファイルはありません。", 13); empty.setTextColor(Color.rgb(100,116,139)); target.addView(empty); return; } for (int i = 0; i < files.length(); i++) { JSONObject f = files.optJSONObject(i); if (f == null) continue; long fileId = f.optLong("id"); LinearLayout row = new LinearLayout(this); row.setOrientation(LinearLayout.VERTICAL); row.setPadding(dp(12), dp(10), dp(12), dp(10)); row.setBackground(rounded(Color.rgb(248,250,255), Color.rgb(226,232,240), 8)); if (isImageFile(f.optString("file_path"))) { ImageView thumbnail = thumbnailView(); row.addView(thumbnail, new LinearLayout.LayoutParams(-1, dp(112))); thumbnail.setOnClickListener(v -> showImagePreview(f)); loadThumbnail(thumbnail, f); } TextView meta = text("アップロード日時: " + f.optString("created_at", "未登録"), 11); meta.setTextColor(Color.rgb(100,116,139)); if (selectable) { CheckBox box = new CheckBox(this); box.setText(f.optString("file_path") + "  (" + f.optLong("file_size") + " bytes)"); box.setTextSize(14); selectedFiles.put(fileId, box); row.addView(box); row.addView(meta); } else { TextView label = text(f.optString("file_path") + "  (" + f.optLong("file_size") + " bytes)", 14); label.setTypeface(null, 1); row.addView(label); row.addView(meta); Button open = secondaryButton("開く"); row.addView(open); open.setOnClickListener(v -> openOfflineFile(fileId, f.optString("file_path"))); } LinearLayout.LayoutParams rowParams = new LinearLayout.LayoutParams(-1, -2); rowParams.setMargins(0, dp(6), 0, 0); target.addView(row, rowParams); } }

    private void renderPictureGrid(LinearLayout target, JSONArray files, boolean selectable) {
        if (files == null || files.length() == 0) {
            TextView empty = text("登録されている写真がありません。", 13);
            empty.setTextColor(Color.rgb(100,116,139));
            target.addView(empty);
            return;
        }
        LinearLayout row = null;
        int visibleIndex = 0;
        for (int i = 0; i < files.length(); i++) {
            JSONObject f = files.optJSONObject(i);
            if (f == null) continue;
            if (visibleIndex % 2 == 0) {
                row = new LinearLayout(this);
                row.setOrientation(LinearLayout.HORIZONTAL);
                target.addView(row, new LinearLayout.LayoutParams(-1, -2));
            }
            LinearLayout card = new LinearLayout(this);
            card.setOrientation(LinearLayout.VERTICAL);
            card.setPadding(dp(7), dp(7), dp(7), dp(7));
            card.setBackground(rounded(Color.rgb(248,250,255), Color.rgb(226,232,240), 8));
            ImageView thumbnail = thumbnailView();
            card.addView(thumbnail, new LinearLayout.LayoutParams(-1, dp(112)));
            if (isImageFile(f.optString("file_path"))) {
                thumbnail.setOnClickListener(v -> showImagePreview(f));
                loadThumbnail(thumbnail, f);
            } else {
                thumbnail.setImageResource(android.R.drawable.ic_menu_gallery);
            }
            TextView label = text(f.optString("file_path"), 12);
            label.setMaxLines(2);
            label.setEllipsize(android.text.TextUtils.TruncateAt.END);
            card.addView(label);
            String tag = f.optString("tag", "").trim();
            TextView tagLabel = text(tag.isEmpty() ? "タグ未設定" : "タグ: " + tag, 12);
            tagLabel.setTextColor(tag.isEmpty() ? Color.rgb(100,116,139) : Color.rgb(39,88,184));
            card.addView(tagLabel);
            if (selectable) {
                Button editTag = secondaryButton(tag.isEmpty() ? "タグを追加" : "タグを編集");
                editTag.setTextSize(12);
                card.addView(editTag);
                editTag.setOnClickListener(v -> showTagDialog(f));
                CheckBox box = new CheckBox(this);
                box.setText("保存");
                box.setTextSize(12);
                selectedFiles.put(f.optLong("id"), box);
                card.addView(box);
            } else {
                Button open = secondaryButton("開く");
                card.addView(open);
                open.setOnClickListener(v -> openOfflineFile(f.optLong("id"), f.optString("file_path")));
            }
            if (row != null) row.addView(card, new LinearLayout.LayoutParams(0, -2, 1));
            visibleIndex++;
        }
        if (visibleIndex % 2 == 1 && row != null) row.addView(new Space(this), new LinearLayout.LayoutParams(0, dp(1), 1));
    }
    private void showTagDialog(JSONObject file) {
        EditText input = input("タグ");
        input.setText(file.optString("tag", ""));
        input.setSelectAllOnFocus(true);
        new AlertDialog.Builder(this)
            .setTitle("写真タグ")
            .setMessage("写真に付けるタグを入力してください。空欄でタグを削除できます。")
            .setView(input)
            .setNegativeButton("キャンセル", null)
            .setPositiveButton("保存", (dialog, which) -> {
                String tag = input.getText().toString().trim();
                runAsync(() -> {
                    api.updateTag(file.optLong("id"), tag);
                    JSONObject detail = api.detail(currentProjectId);
                    runOnUiThread(() -> showDetail(detail, false));
                });
            })
            .show();
    }
    private ImageView thumbnailView() { ImageView view = new ImageView(this); view.setScaleType(ImageView.ScaleType.CENTER_CROP); view.setBackground(rounded(Color.rgb(226,232,240), 0, 6)); view.setContentDescription("画像サムネイル"); return view; }
    private void loadThumbnail(ImageView target, JSONObject file) { long fileId = file.optLong("id"); runAsync(() -> { try { byte[] bytes; if (offlineMode) { File local = offlineCache.fileFor(api.getBaseUrl(), currentProjectId, fileId); if (!local.isFile()) return; bytes = java.nio.file.Files.readAllBytes(local.toPath()); } else { bytes = api.downloadBytes(fileId); } Bitmap bitmap = BitmapFactory.decodeByteArray(bytes, 0, bytes.length); if (bitmap != null) runOnUiThread(() -> target.setImageBitmap(bitmap)); } catch (Exception ignored) { } }); }
    private boolean isImageFile(String name) { String value = extension(name); return Arrays.asList("jpg", "jpeg", "png", "webp", "gif", "bmp", "heic", "heif", "avif").contains(value); }
    private void showImagePreview(JSONObject file) { runAsync(() -> { try { byte[] bytes; if (offlineMode) { File local = offlineCache.fileFor(api.getBaseUrl(), currentProjectId, file.optLong("id")); if (!local.isFile()) throw new IOException("保存済みファイルがありません。"); bytes = java.nio.file.Files.readAllBytes(local.toPath()); } else { bytes = api.downloadBytes(file.optLong("id")); } Bitmap bitmap = BitmapFactory.decodeByteArray(bytes, 0, bytes.length); if (bitmap == null) throw new IOException("画像を表示できません。"); runOnUiThread(() -> { ImageView image = thumbnailView(); image.setAdjustViewBounds(true); image.setScaleType(ImageView.ScaleType.FIT_CENTER); image.setImageBitmap(bitmap); image.setMinimumHeight(dp(240)); new AlertDialog.Builder(this).setTitle(file.optString("file_path", "画像プレビュー")).setView(image).setPositiveButton("閉じる", null).show(); }); } catch (Exception error) { runOnUiThread(() -> info("画像プレビューを表示できません。")); } }); }
    private void cacheSelectedFiles() { List<Long> ids = new ArrayList<>(); for (Map.Entry<Long, CheckBox> entry : selectedFiles.entrySet()) if (entry.getValue().isChecked()) ids.add(entry.getKey()); if (ids.isEmpty()) { info("保存するファイルを選択してください。"); return; } EditText days = input("保存期間（日）"); days.setInputType(InputType.TYPE_CLASS_NUMBER); days.setText(String.valueOf(offlineCache.retentionDays())); days.setSelectAllOnFocus(true); new AlertDialog.Builder(this).setTitle("オフライン保存期間").setMessage("選択したファイルの保存期間を1〜3650日で指定してください。").setView(days).setNegativeButton("キャンセル", null).setPositiveButton("保存", (dialog, which) -> { try { int value = Integer.parseInt(days.getText().toString().trim()); if (value < 1 || value > 3650) throw new NumberFormatException(); offlineCache.setRetentionDays(value); saveSelectedFiles(ids); } catch (NumberFormatException error) { info("保存期間は1〜3650日で指定してください。"); } }).show(); }
    private void saveSelectedFiles(List<Long> ids) { runAsync(() -> { Map<Long, byte[]> bytes = new LinkedHashMap<>(); for (long fileId : ids) bytes.put(fileId, api.downloadBytes(fileId)); offlineCache.saveProject(api.getBaseUrl(), currentDetail, bytes); runOnUiThread(() -> info(ids.size() + "件をオフライン表示用に保存しました。")); }); }
    private void openOfflineFile(long fileId, String name) { try { File file = offlineCache.fileFor(api.getBaseUrl(), currentProjectId, fileId); if (!file.isFile()) throw new Exception("保存済みファイルがありません。"); Uri uri = FileProvider.getUriForFile(this, getPackageName() + ".fileprovider", file); Intent intent = new Intent(Intent.ACTION_VIEW, uri); String mime = MimeTypeMap.getSingleton().getMimeTypeFromExtension(extension(name)); intent.setDataAndType(uri, mime == null ? "application/octet-stream" : mime); intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION); startActivity(intent); } catch (Exception e) { info("このファイルを開けるアプリがありません。"); } }
    private String extension(String name) { int dot = name.lastIndexOf('.'); return dot < 0 ? "bin" : name.substring(dot + 1).toLowerCase(Locale.ROOT); }
    @Override protected void onActivityResult(int requestCode, int resultCode, Intent data) { super.onActivityResult(requestCode, resultCode, data); if (requestCode != PICK_FILE || resultCode != RESULT_OK || data == null || data.getData() == null) return; Uri uri = data.getData(); String name = uri.getLastPathSegment(); String mime = getContentResolver().getType(uri); String category = pendingUploadCategory; runAsync(() -> { api.upload(currentProjectId, getContentResolver(), uri, name == null ? "upload.bin" : name, mime, category); JSONObject d = api.detail(currentProjectId); runOnUiThread(() -> showDetail(d, false)); }); }
}
