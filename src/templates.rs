use crate::core::can_manage_projects;
use crate::handlers::auth_handlers::authenticate_request;
use crate::models::{ApiError, AppState};
use axum::{
    extract::{Path, State},
    http::HeaderMap,
    response::Html,
};
use std::sync::Arc;

pub const LOGIN_HTML: &str = include_str!("templates/login.html");

pub const ADMIN_HTML: &str = include_str!("templates/admin.html");

pub const ADMIN_PROJECTS_HTML: &str = include_str!("templates/admin_projects.html");

pub const USER_REGISTRATION_HTML: &str = include_str!("templates/user_registration.html");

pub const DEALER_REGISTRATION_HTML: &str = include_str!("templates/dealer_registration.html");

pub const INDEX_HTML: &str = include_str!("templates/index.html");

pub const PROJECT_RESULTS_HTML: &str = INDEX_HTML;

pub const DETAIL_HTML: &str = include_str!("templates/detail.html");

pub const HELP_HTML: &str = include_str!("templates/help.html");
pub const APP_VERSION: &str = "v20260915.01";

const MOBILE_LIST_STYLES: &str = r#"<style>
.mobile-search-sort{display:none}
.app-page{display:flex;flex-direction:column;min-height:100vh}
.app-page>.app{flex:1;min-height:0}
.app-page>.page{flex:1;min-height:0;width:100%}
.main{display:flex;flex-direction:column}
.app-copyright{margin-top:auto;padding:12px 24px calc(16px + env(safe-area-inset-bottom));color:var(--muted);font-size:11px;letter-spacing:.02em;text-align:center}
@media(min-width:821px){.main>.content{max-width:none;width:100%;margin-left:0;margin-right:0}}
@media(min-width:821px){.detail-location{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:16px}.detail-location>div+div{margin-top:0!important}}
@media(max-width:560px){
  .table{display:block;width:100%;overflow:visible;white-space:normal;border-collapse:separate;border-spacing:0}
  .table thead{display:none}
  .table tbody{display:block}
  .table tbody tr{display:flex;flex-direction:column;width:100%;margin:0 0 12px;padding:12px 14px;border:1px solid var(--line);border-radius:10px;background:#fff}
  .table tbody td{display:flex;align-items:flex-start;gap:10px;width:100%;padding:8px 0;border-bottom:1px solid var(--line);white-space:normal;overflow-wrap:anywhere}
  .table tbody td:last-child{border-bottom:0}
  .table tbody td::before{content:attr(data-label);flex:0 0 7em;color:var(--muted);font-size:11px;font-weight:700}
  .table tbody td[colspan]{display:block;text-align:center;border-bottom:0;padding:14px 0}
  .table tbody td[colspan]::before{display:none;content:none}
  .table tbody td[data-label="操作"]{display:block}
  .table tbody td[data-label="操作"]::before{display:block;margin-bottom:8px}
  .table tbody td[data-label="操作"] .btn-sm{margin:0 6px 6px 0}
  .file{flex-direction:column;align-items:stretch;gap:8px}
  .file .photo-meta{margin:0!important}
  .file .photo-menu-btn{position:absolute;right:12px;top:12px}
  .file .photo-menu{top:42px}
  .photo-grid{grid-template-columns:1fr;gap:12px}
  .table tbody#results tr{margin-bottom:4px;padding:6px 8px}
  .table tbody#results td{padding:4px 0!important}
  .table tbody#results td::before{flex-basis:6em}
  .mobile-search-sort{display:inline-flex!important;align-items:center;gap:4px;color:var(--muted);font-size:11px;font-weight:700;white-space:nowrap}
  .mobile-search-sort select{width:auto;min-height:32px;height:32px;padding:0 6px;border:1px solid var(--line);border-radius:6px;background:#fff;color:var(--navy);font:inherit;font-weight:600}
}
</style></head>"#;

const MOBILE_LIST_SCRIPT: &str = r#"<script>
(function(){
  function applyResponsiveTableLabels(){
    document.querySelectorAll('table.table').forEach(function(table){
      var labels=Array.from(table.querySelectorAll('thead th')).map(function(th){return th.textContent.trim()});
      table.querySelectorAll('tbody tr').forEach(function(row){
        Array.from(row.cells).forEach(function(cell,index){
          if(cell.colSpan>1)return;
          cell.dataset.label=labels[index]||'';
        });
      });
    });
  }
  applyResponsiveTableLabels();
  new MutationObserver(applyResponsiveTableLabels).observe(document.body,{childList:true,subtree:true});
})();
</script></body>"#;

const IDLE_LOGOUT_SCRIPT: &str = r#"<script>
(function(){
  if(!document.querySelector('.app'))return;
  var timeout=60*60*1000,lastActivity=Date.now(),timer,keepaliveAt=0;
  function logout(){location.href='/logout'}
  function schedule(){clearTimeout(timer);timer=setTimeout(function(){if(Date.now()-lastActivity>=timeout){logout()}else{schedule()}},timeout)}
  function keepalive(){var now=Date.now();if(now-keepaliveAt<5*60*1000)return;keepaliveAt=now;fetch('/api/session/keepalive',{method:'POST',credentials:'same-origin',keepalive:true}).catch(function(){})}
  function activity(){lastActivity=Date.now();schedule();keepalive()}
  ['pointerdown','keydown','input','scroll'].forEach(function(type){document.addEventListener(type,activity,{passive:true})});
  document.addEventListener('visibilitychange',function(){if(!document.hidden&&Date.now()-lastActivity>=timeout)logout()});
  schedule();
})();
</script></body>"#;

const WEB_OFFLINE_REMOVAL_SCRIPT: &str = r#"<script>
(function(){
  try {
    if (navigator.serviceWorker) {
      navigator.serviceWorker.getRegistrations().then(function(registrations){
        registrations.forEach(function(registration){ registration.unregister(); });
      }).catch(function(){});
    }
    if (window.caches) {
      caches.keys().then(function(keys){
        keys.filter(function(key){ return key.indexOf("filemanager3-offline-shell-") === 0; })
          .forEach(function(key){ caches.delete(key); });
      }).catch(function(){});
    }
    if (window.indexedDB) indexedDB.deleteDatabase("filemanager3-offline-v1");
  } catch (_) {}
})();
</script></body>"#;

const FILE_AUDIT_SCRIPT: &str = r#"<script>
(function(){
  function escapeHtml(value){return String(value||'').replace(/[&<>\"']/g,function(c){return {'&':'&amp;','<':'&lt;','>':'&gt;','\"':'&quot;',"'":'&#39;'}[c]})}
  window.openFileAudit=async function(fileId){
    try{
      var response=await fetch('/api/files/'+fileId+'/audit',{cache:'no-store'});
      if(!response.ok)throw new Error(await response.text());
      var items=await response.json();
      var rows=items.length?items.map(function(item){return '<tr><td>第'+item.version_number+'版</td><td>'+escapeHtml(item.file_path)+'</td><td>'+escapeHtml(item.uploaded_by||'不明')+'<br><span class=\"panel-sub\">'+escapeHtml(item.uploaded_at)+'</span></td><td>'+escapeHtml(item.deleted_by||'未削除')+(item.deleted_at?'<br><span class=\"panel-sub\">'+escapeHtml(item.deleted_at)+'</span>':'')+'</td></tr>'}).join(''):'<tr><td colspan=\"4\" class=\"no-file\">監査情報はありません。</td></tr>';
      var backdrop=document.createElement('div');
      backdrop.className='modal-backdrop';
      backdrop.dataset.fileAuditDialog='1';
      backdrop.innerHTML='<section class=\"modal\" role=\"dialog\" aria-modal=\"true\" aria-labelledby=\"file-audit-title\" style=\"max-width:760px;overflow:auto\"><div class=\"modal-head\"><h2 id=\"file-audit-title\">ファイル操作ユーザー</h2><button class=\"icon-btn\" type=\"button\" data-file-audit-close aria-label=\"閉じる\">×</button></div><p class=\"panel-sub\">管理者のみ確認できます。</p><table class=\"table\"><thead><tr><th>版</th><th>ファイル</th><th>アップロード</th><th>論理削除</th></tr></thead><tbody>'+rows+'</tbody></table></section>';
      backdrop.querySelector('[data-file-audit-close]').onclick=function(){backdrop.remove()};
      backdrop.onclick=function(event){if(event.target===backdrop)backdrop.remove()};
      document.body.appendChild(backdrop);
    }catch(error){alert(error.message||'ファイル操作ユーザーを取得できませんでした。')}
  };
  document.addEventListener('click',function(event){var button=event.target.closest('[data-file-audit]');if(!button)return;event.preventDefault();event.stopPropagation();openFileAudit(Number(button.dataset.fileAudit))});
})();
</script></body>"#;

pub fn render_page(html: &str) -> String {
    html.replace("v20260915.00", APP_VERSION)
        .replace("v20260915.01", APP_VERSION)
        .replace("<body>", "<body class=\"app-page\">")
        .replace("</head>", MOBILE_LIST_STYLES)
        .replace(
            "</main></div></div>",
            "</main><footer class=\"app-copyright\">© 2026 FileManager3</footer></div></div>",
        )
        .replace(
            "</main><script>",
            "</main><footer class=\"app-copyright\">© 2026 FileManager3</footer><script>",
        )
        .replace(
            "class=\"nav-item admin-only-nav\" href=\"/admin/users\"",
            "class=\"nav-item user-self-nav\" href=\"/admin/users\"",
        )
        .replace(
            "</h2><div style=\"font-size:13px;color:var(--muted)\">該当件数:",
            "</h2><label class=\"mobile-search-sort\" for=\"mobile-search-sort\">ソート<select id=\"mobile-search-sort\" aria-label=\"検索結果のソート\"><option value=\"updated_desc\">更新日時 ↓</option><option value=\"updated_asc\">更新日時 ↑</option><option value=\"number_asc\">案件番号 ↑</option><option value=\"number_desc\">案件番号 ↓</option><option value=\"name_asc\">案件名 ↑</option><option value=\"name_desc\">案件名 ↓</option></select></label><div style=\"font-size:13px;color:var(--muted)\">該当件数:",
        )
        .replace(
            "<label for=\"password\">パスワード</label><input id=\"password\" name=\"password\" type=\"password\" autocomplete=\"current-password\" required>",
            "<label for=\"password\">パスワード</label><div class=\"password-field\" style=\"position:relative;display:block\"><input id=\"password\" name=\"password\" type=\"password\" autocomplete=\"current-password\" required><button id=\"password-toggle\" class=\"password-toggle\" type=\"button\" aria-label=\"パスワードを表示\" style=\"position:absolute;right:0;top:0\">表示</button></div>",
        )
        .replace(
            "<label>パスワード（8文字以上）</label><input name=\"password\" type=\"password\" required minlength=\"8\">",
            "<label>パスワード（8文字以上）</label><div class=\"password-field\" style=\"position:relative;display:block\"><input id=\"user-password\" name=\"password\" type=\"password\" required minlength=\"8\"><button id=\"user-password-toggle\" class=\"password-toggle\" type=\"button\" aria-label=\"パスワードを表示\" style=\"position:absolute;right:0;top:0\" onclick=\"const i=document.getElementById('user-password');const v=i.type==='text';i.type=v?'password':'text';this.textContent=v?'表示':'非表示';this.setAttribute('aria-label',v?'パスワードを表示':'パスワードを非表示');i.focus()\">表示</button></div>",
        )
        .replace(
            "<button class=\"btn\" onclick=\"openUserCreate()\">＋ 新規ユーザー登録</button>",
            "<button id=\"user-create-button\" class=\"btn\" onclick=\"openUserCreate()\">＋ 新規ユーザー登録</button>",
        )
        .replace(
            "<div class=\"form-grid\" style=\"grid-template-columns:1fr;gap:8px\"><div><label>ユーザー名</label><input name=\"username\" required></div><div><label>パスワード（8文字以上）</label><input name=\"password\" type=\"password\" required minlength=\"8\"></div><div><label>ロール</label><select name=\"role\">",
            "<div class=\"form-grid\" style=\"grid-template-columns:1fr;gap:8px\"><div><label>ユーザー名</label><input name=\"username\" required></div><div><label>パスワード（8文字以上）</label><input name=\"password\" type=\"password\" required minlength=\"8\"></div><div id=\"role-field\"><label>ロール</label><select name=\"role\">",
        )
        .replace(
            "let userEditId=null;function openUserCreate(){",
            "let userEditId=null,currentUserId=null,currentUserRole='';function applyUserManagementUi(){var viewer=currentUserRole==='viewer',create=document.querySelector('#user-create-button'),roleField=document.querySelector('#role-field');if(create)create.style.display=viewer?'none':'';if(roleField)roleField.style.display=viewer?'none':''}async function loadCurrentUser(){var res=await fetch('/api/me',{cache:'no-store'});if(!res.ok)throw new Error('ログイン情報を取得できませんでした。');var me=await res.json();currentUserId=me.id;currentUserRole=me.role||'';applyUserManagementUi();new MutationObserver(adjustUserActions).observe(userList,{childList:true})}function adjustUserActions(){userList.querySelectorAll('tr').forEach(function(row){if(currentUserRole==='viewer'){row.querySelectorAll('button').forEach(function(button){if(!button.textContent.includes('編集'))button.remove()})}else if(currentUserRole==='admin'&&row.cells[0]&&row.cells[0].textContent.trim()===String(currentUserId)){var button=row.querySelector('button[onclick^=\"deleteUser(\"]');if(button)button.remove()}})}function openUserCreate(){if(currentUserRole!=='admin')return;",
        )
        .replace(
            "const payload=userEditId?{role:data.role}:{username:data.username,password:data.password,role:data.role};",
            "const payload=userEditId?(currentUserRole==='viewer'?{}:{role:data.role}):{username:data.username,password:data.password,role:data.role};",
        )
        .replace(
            "loadUsers();</script>",
            "loadCurrentUser().then(function(){return loadUsers()}).then(adjustUserActions).catch(function(error){userList.innerHTML='<tr><td colspan=\"6\" style=\"text-align:center;color:#b91c1c\">'+esc(error.message)+'</td></tr>'});</script>",
        )
        .replace(
            "</style></head>",
            "<style>.password-field{position:relative;display:block}.password-field input{width:100%;padding-right:72px}.password-toggle{position:absolute;right:0;top:0;width:auto;height:44px;margin:0;padding:0 12px;background:#fff;color:#3977e8;border:1px solid #9bbcf5;font-size:12px;white-space:nowrap}.password-toggle:hover{background:#f8faff}.top-access-url,.access-info{display:none!important}.sidebar .nav-submenu,.sidebar .specification-submenu{display:none}.specification-submenu{margin:0 0 6px 30px;border-left:1px solid #36506f;padding-left:4px}.specification-submenu a{display:block;padding:5px 8px;color:#91a5c0;font-size:11px;line-height:1.35}.specification-submenu a:hover{color:#fff}.admin-toggle-icon{margin-left:auto;font-size:10px;color:#8294b1;padding-right:4px}.email-link{display:inline-flex;align-items:center;color:#3977e8;font-weight:700;text-decoration:none}.email-link:hover{text-decoration:underline}</style></head>",
        )
        .replace(
            r#"function phoneAnchor(value,label){const phone=String(value||'').replace(/[^0-9+]/g,'');return phone?'<a class="phone-link" href="tel:'+encodeURIComponent(phone)+'">📞 '+esc(label||value)+'</a>':esc(label||'未登録')}"#,
            r#"function phoneAnchor(value,label){const phone=String(value||'').replace(/[^0-9+]/g,'');return phone?'<a class="phone-link" href="tel:'+encodeURIComponent(phone)+'">📞 '+esc(label||value)+'</a>':esc(label||'未登録')}function emailAnchor(value){const email=String(value||'').trim();return email?'<a class="email-link" href="mailto:'+encodeURIComponent(email)+'">'+esc(email)+'</a>':'未登録'}"#,
        )
        .replace(
            r#"<label>メールアドレス</label><strong>${esc(detail.project.email||"未登録")}</strong>"#,
            r#"<label>メールアドレス</label><strong>${emailAnchor(detail.project.email)}</strong>"#,
        )
        .replace(
            "<td>${esc(c.email || '未登録')}</td>",
            r#"<td>${c.email?`<a class="email-link" href="mailto:${encodeURIComponent(c.email)}">${esc(c.email)}</a>`:'未登録'}</td>"#,
        )
        .replace(
            "<td>${esc(d.email||'未登録')}</td>",
            r#"<td>${d.email?`<a class="email-link" href="mailto:${encodeURIComponent(d.email)}">${esc(d.email)}</a>`:'未登録'}</td>"#,
        )
        .replace(
            "if(location.hostname==='127.0.0.1')",
            "const passwordInput=document.querySelector('#password'),passwordToggle=document.querySelector('#password-toggle');passwordToggle&&passwordToggle.addEventListener('click',()=>{const visible=passwordInput.type==='text';passwordInput.type=visible?'password':'text';passwordToggle.textContent=visible?'表示':'非表示';passwordToggle.setAttribute('aria-label',visible?'パスワードを表示':'パスワードを非表示');passwordInput.focus()});if(location.hostname==='127.0.0.1')",
        )
        .replace(
            "catch(_){}form.username.addEventListener(\"input\"",
            "catch(_){} }form.username.addEventListener(\"input\"",
        )
        .replace(
            "function setTopUserName(username,role){",
            "function topEscape(v){return String(v).replace(/[&<>\"']/g,function(c){return {'&':'&amp;','<':'&lt;','>':'&gt;','\"':'&quot;',\"'\":'&#39;'}[c]})}function ensureSpecificationsLink(){var specs=[['01_overview','システム概要'],['02_permissions','権限仕様'],['03_screens','画面仕様'],['04_api','API仕様'],['05_architecture_and_data','アーキテクチャ・データ構造'],['06_configuration','設定・起動'],['07_android','Android版'],['08_ios','iOS版'],['09_ubuntu_letsencrypt',\"Ubuntu・Let's Encrypt 設定\"],['10_docker_postgresql','Docker PostgreSQL']];document.querySelectorAll('.nav-submenu').forEach(function(menu){var link=menu.querySelector('a[href=\"/admin/specifications\"]');if(!link){var dealer=menu.querySelector('a[href=\"/admin/dealers\"]');if(!dealer)return;link=document.createElement('a');link.className='nav-item admin-only-nav';link.href='/admin/specifications';link.innerHTML='<span>▧</span><span>仕様書</span>';dealer.after(link)}if(menu.querySelector('.specification-submenu'))return;var submenu=document.createElement('div');submenu.className='specification-submenu admin-only-nav';submenu.innerHTML=specs.map(function(spec){return '<a href=\"/admin/specifications/'+spec[0]+'\">'+spec[1]+'</a>'}).join('');link.after(submenu)})}function setupMenuToggles(){var adminMenuLink=document.querySelector('.sidebar a.nav-item[href=\"/admin\"]');var submenu=document.querySelector('.sidebar .nav-submenu');if(adminMenuLink&&submenu){if(!adminMenuLink.querySelector('.admin-toggle-icon')){var icon=document.createElement('span');icon.className='admin-toggle-icon';adminMenuLink.appendChild(icon)}var toggleIcon=adminMenuLink.querySelector('.admin-toggle-icon');var stored=sessionStorage.getItem('fm3_admin_menu_open');var isOpen=stored==='1';function updateAdminState(open){submenu.style.display=open?'block':'none';if(toggleIcon)toggleIcon.textContent=open?'▲':'▼';sessionStorage.setItem('fm3_admin_menu_open',open?'1':'0')}updateAdminState(isOpen);if(!adminMenuLink.dataset.toggleBound){adminMenuLink.dataset.toggleBound='1';adminMenuLink.addEventListener('click',function(e){e.preventDefault();document.querySelectorAll('.sidebar a.nav-item').forEach(function(item){item.classList.remove('active')});adminMenuLink.classList.add('active');var currentOpen=submenu.style.display!=='none';updateAdminState(!currentOpen)})}}var specLink=document.querySelector('.sidebar a[href=\"/admin/specifications\"]');var specSubmenu=document.querySelector('.sidebar .specification-submenu');if(specLink&&specSubmenu){if(!specLink.querySelector('.spec-toggle-icon')){var sIcon=document.createElement('span');sIcon.className='spec-toggle-icon admin-toggle-icon';specLink.appendChild(sIcon)}var specToggleIcon=specLink.querySelector('.spec-toggle-icon');var specStored=sessionStorage.getItem('fm3_spec_menu_open');var isSpecOpen=specStored==='1';function updateSpecState(open){specSubmenu.style.display=open?'block':'none';if(specToggleIcon)specToggleIcon.textContent=open?'▲':'▼';sessionStorage.setItem('fm3_spec_menu_open',open?'1':'0')}updateSpecState(isSpecOpen);if(!specLink.dataset.toggleBound){specLink.dataset.toggleBound='1';specLink.addEventListener('click',function(e){e.preventDefault();var currentSpecOpen=submenu.style.display!=='none';updateSpecState(!currentSpecOpen)})}}}function setTopUserName(username,role){",
        )
        .replace(
            "title=\"'+username+'\">'+username+'</span>",
            "title=\"'+topEscape(username)+'\">'+topEscape(username)+'</span>",
        )
        .replace(
            "function setAdminMenuVisibility(role){var canManage=role===\"admin\"||role===\"member\";document.querySelectorAll(\".admin-nav-link\").forEach(function(e){e.style.display=canManage?(e.classList.contains(\"nav-submenu\")?\"block\":\"flex\"):\"none\"});",
            "function setAdminMenuVisibility(role){var canManage=role===\"admin\"||role===\"member\";document.querySelectorAll(\".admin-nav-link\").forEach(function(e){if(!canManage){e.style.display=\"none\"}else if(!e.classList.contains(\"nav-submenu\")){e.style.display=\"flex\"}});if(role===\"viewer\"){document.querySelectorAll(\".admin-nav-link.nav-submenu\").forEach(function(e){e.style.display=\"block\"});document.querySelectorAll(\".admin-nav-link .nav-item:not(.user-self-nav)\").forEach(function(e){e.style.display=\"none\"})}document.querySelectorAll(\".user-self-nav\").forEach(function(e){e.style.display=role===\"admin\"||role===\"viewer\"?\"flex\":\"none\"});",
        )
        .replace(
            "setTopUserName(topCacheGet(\"fm3_top_username\"));setTopAccessAddress(topCacheGet(\"fm3_top_access_url\"));setAdminMenuVisibility(topCacheGet(\"fm3_top_role\"));",
            "setTopUserName(topCacheGet(\"fm3_top_username\"));ensureSpecificationsLink();setupMenuToggles();setTopAccessAddress(topCacheGet(\"fm3_top_access_url\"));setAdminMenuVisibility(topCacheGet(\"fm3_top_role\"));",
        )
        .replace(
            "setAdminMenuVisibility(d.role)",
            "setupMenuToggles();setAdminMenuVisibility(d.role)",
        )
        .replace(
            "openUserEdit(${u.id},'${esc(u.username)}','${roleLabel(u.role)}')",
            "openUserEdit(${u.id},'${jsEsc(u.username)}','${u.role}')",
        )
        .replace(
            "deleteUser(${u.id}, '${esc(u.username)}')",
            "deleteUser(${u.id}, '${jsEsc(u.username)}')",
        )
        .replace(
            "function roleLabel(role){",
            "function jsEsc(v){return String(v).replace(/[\\\\'\"\\n\\r&<>]/g,function(c){return {'\\\\':'\\\\\\\\',\"'\":\"\\\\'\",'\"':'&quot;','&':'&amp;','<':'&lt;','>':'&gt;','\\n':'\\\\n','\\r':'\\\\r'}[c]})}function roleLabel(role){",
        )
        .replace(
            "</body>",
            "<script>(function(){var previousFocus=null;function visibleDialog(){return Array.from(document.querySelectorAll('.modal-backdrop,.dialog-backdrop')).find(function(el){return getComputedStyle(el).display!=='none'})}function focusDialog(dialog){previousFocus=document.activeElement;var target=dialog.querySelector('input,select,textarea,button,[tabindex]:not([tabindex=\"-1\"])');if(target)target.focus()}function closeDialog(dialog){var close=dialog.querySelector('[data-dialog-close],[data-note-close],[data-deleted-notes-close],.dialog-close');if(close)close.click();else if(dialog.dataset)dialog.remove();if(previousFocus&&previousFocus.isConnected)previousFocus.focus();previousFocus=null}document.addEventListener('keydown',function(e){var dialog=visibleDialog();if(!dialog)return;if(e.key==='Escape'){e.preventDefault();closeDialog(dialog);return}if(e.key!=='Tab')return;var focusables=Array.from(dialog.querySelectorAll('button,input,select,textarea,a[href],[tabindex]:not([tabindex=\"-1\"])')).filter(function(el){return !el.disabled&&getComputedStyle(el).display!=='none'});if(!focusables.length)return;var first=focusables[0],last=focusables[focusables.length-1];if(e.shiftKey&&document.activeElement===first){e.preventDefault();last.focus()}else if(!e.shiftKey&&document.activeElement===last){e.preventDefault();first.focus()}});new MutationObserver(function(records){records.forEach(function(record){record.addedNodes.forEach(function(node){if(node.nodeType===1&&(node.matches('.modal-backdrop,.dialog-backdrop')||node.querySelector('.modal,.form-panel'))){setTimeout(function(){focusDialog(node.matches('.modal-backdrop,.dialog-backdrop')?node:node.querySelector('.modal,.form-panel').parentElement)},0)}})})}).observe(document.body,{childList:true,subtree:true});document.addEventListener('click',function(e){if(e.target.closest('.top-logout')){try{sessionStorage.removeItem('fm3_top_username');sessionStorage.removeItem('fm3_top_role');sessionStorage.removeItem('fm3_top_access_url')}catch(_){} }});})();</script></body>",
        )
        .replace(
            "</body>",
            "<script>document.addEventListener('click',function(e){var move=e.target.closest('[data-tag-edit]');if(!move)return;e.preventDefault();var fileId=Number(move.dataset.tagEdit),currentTag=move.dataset.currentTag||'';if(typeof openTagEditDialog==='function')openTagEditDialog(currentTag,function(tag){return moveFiles([fileId],tag)})});</script></body>",
        )
        .replace(
            "</body>",
            r#"<script>(function(){function escapeHtml(value){return String(value||'').replace(/[&<>"']/g,function(c){return {'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]})}function addPhysicalButton(row,url){if(!row||row.dataset.physicalDeleteAdded)return;var restore=row.querySelector('button[onclick*="restore"]');if(!restore)return;var match=(restore.getAttribute('onclick')||'').match(/\((\d+)\)/);if(!match)return;var button=document.createElement('button');button.className='btn-sm danger';button.type='button';button.textContent='完全削除';button.dataset.physicalDelete='1';button.onclick=async function(){if(!confirm('完全削除すると復元できません。実行しますか？'))return;try{var res=await fetch(url(match[1]),{method:'DELETE'});if(!res.ok)throw new Error(await res.text());row.remove()}catch(error){alert(error.message||'物理削除に失敗しました。')}};restore.after(document.createTextNode(' '),button);row.dataset.physicalDeleteAdded='1'}function observeTrash(selector,url){var list=document.querySelector(selector);if(!list)return;var update=function(){list.querySelectorAll('tr').forEach(function(row){addPhysicalButton(row,url)})};update();new MutationObserver(update).observe(list,{childList:true})}function addDeletedNoteButtons(){document.querySelectorAll('.modal-backdrop .file').forEach(function(row){if(row.dataset.physicalDeleteAdded)return;var restore=row.querySelector('[data-note-restore],[data-dealer-note-restore]');if(!restore)return;var projectNoteId=restore.dataset.noteRestore;var dealerNoteId=restore.dataset.dealerNoteRestore;var noteId=projectNoteId||dealerNoteId;if(!noteId)return;var dealerId=typeof activeDealerId!=='undefined'?activeDealerId:'';var endpoint=projectNoteId?'/api/projects/'+projectIdFromPath()+'/notes/'+noteId+'/permanent':'/api/dealers/'+dealerId+'/notes/'+noteId+'/permanent';var button=document.createElement('button');button.className='btn-sm danger';button.type='button';button.textContent='完全削除';button.onclick=async function(){if(!confirm('完全削除すると復元できません。実行しますか？'))return;try{var res=await fetch(endpoint,{method:'DELETE'});if(!res.ok)throw new Error(await res.text());row.remove()}catch(error){alert(error.message||'物理削除に失敗しました。')}};restore.after(document.createTextNode(' '),button);row.dataset.physicalDeleteAdded='1'})}function projectIdFromPath(){var match=location.pathname.match(/\/projects\/(\d+)/);return match?match[1]:''}function initDeletedUsers(){var list=document.querySelector('#user-list'),head=document.querySelector('.head-row');if(!list||!head||document.querySelector('#deleted-users-panel'))return;var toggle=document.createElement('button');toggle.className='btn secondary deleted-toggle';toggle.type='button';toggle.textContent='削除済みを表示';head.insertBefore(toggle,head.lastElementChild);var panel=document.createElement('div');panel.id='deleted-users-panel';panel.style.cssText='display:none;margin-top:28px';panel.innerHTML='<h2 style="font-size:16px;margin:0 0 12px">削除済みユーザー</h2><table class="table"><thead><tr><th>ID</th><th>ユーザー名</th><th>ロール</th><th>登録日時</th><th>操作</th></tr></thead><tbody><tr><td colspan="5" style="text-align:center;color:var(--muted)">読み込み中…</td></tr></tbody></table>';list.closest('table').after(panel);var deletedList=panel.querySelector('tbody');async function load(){try{var res=await fetch('/api/deleted/users',{cache:'no-store'});if(!res.ok)throw new Error(await res.text());var users=await res.json();deletedList.innerHTML=users.length?users.map(function(user){return '<tr><td>'+user.id+'</td><td><strong>'+escapeHtml(user.username)+'</strong></td><td>'+escapeHtml(user.role)+'</td><td>'+escapeHtml(user.created_at||'')+'</td><td><button class="btn-sm danger" type="button" data-physical-user="'+user.id+'">完全削除</button></td></tr>'}).join(''):'<tr><td colspan="5" style="text-align:center;color:var(--muted)">削除済みユーザーはありません。</td></tr>'}catch(error){deletedList.innerHTML='<tr><td colspan="5" style="text-align:center;color:#b91c1c">削除済みユーザーの取得に失敗しました。</td></tr>'}}toggle.onclick=function(){var visible=panel.style.display==='block';panel.style.display=visible?'none':'block';toggle.textContent=visible?'削除済みを表示':'削除済みを非表示';if(!visible)load()};deletedList.addEventListener('click',async function(event){var button=event.target.closest('[data-physical-user]');if(!button||!confirm('完全削除すると復元できません。実行しますか？'))return;try{var res=await fetch('/api/deleted/users/'+button.dataset.physicalUser,{method:'DELETE'});if(!res.ok)throw new Error(await res.text());button.closest('tr').remove()}catch(error){alert(error.message||'物理削除に失敗しました。')}})}observeTrash('#deleted-project-list',function(id){return '/api/deleted/projects/'+id});observeTrash('#deleted-dealer-list',function(id){return '/api/deleted/dealers/'+id});initDeletedUsers();new MutationObserver(addDeletedNoteButtons).observe(document.body,{childList:true,subtree:true});addDeletedNoteButtons()})();</script></body>"#,
        )
        .replace(
            "function fileName(path){",
            "function formatUploadTime(value){return value?String(value):'日時不明'}function fileName(path){",
        )
        .replace(
            r#"function historyAction(file){return currentUserRole==="admin"&&Number(file.version_number)>1?"<button type='button' data-file-history='"+file.id+"'>旧バージョン管理</button>":""}"#,
            r#"function historyAction(file){const audit=currentUserRole==="admin"?"<button type='button' data-file-audit='"+file.id+"'>操作ユーザー</button>":"";return audit+(currentUserRole==="admin"&&Number(file.version_number)>1?"<button type='button' data-file-history='"+file.id+"'>旧バージョン管理</button>":"")}"#,
        )
        .replace(
            "<div class=\"photo-meta\"><span>${formatSize(f.file_size||0)}</span></div>",
            "<div class=\"photo-meta\"><span>${formatSize(f.file_size||0)}</span><span>アップロード: ${formatUploadTime(f.created_at)}</span></div>",
        )
        .replace(
            "<div class=\"photo-meta\"><span>${formatSize(f.file_size||0)}</span>${tag?",
            "<div class=\"photo-meta\"><span>${formatSize(f.file_size||0)}</span><span>アップロード: ${formatUploadTime(f.created_at)}</span>${tag?",
        )
        .replace("<th>登録日時</th>", "<th>更新日時</th>")
        .replace("u.created_at||''", "u.updated_at||''")
        .replace("user.created_at||''", "user.updated_at||''")
        .replace(
            "const sortKeys={number:['number_asc','number_desc'],name:['name_asc','name_desc'],updated:['updated_asc','updated_desc']};",
            "const sortKeys={number:['number_asc','number_desc'],name:['name_asc','name_desc'],updated:['updated_asc','updated_desc']};const mobileSearchSort=document.querySelector('#mobile-search-sort');if(mobileSearchSort){mobileSearchSort.value=queryParams.get('sort')||'updated_desc';mobileSearchSort.addEventListener('change',()=>{queryParams.set('sort',mobileSearchSort.value);history.replaceState(null,'',location.pathname+'?'+queryParams.toString());loadResults()})};",
        )
        .replace("</body>", FILE_AUDIT_SCRIPT)
        .replace("</body>", IDLE_LOGOUT_SCRIPT)
        .replace("</body>", WEB_OFFLINE_REMOVAL_SCRIPT)
        .replace("</body>", MOBILE_LIST_SCRIPT)
}

pub fn login_page_html() -> String {
    login_page_html_with_google(crate::config::google_oauth_config().is_some())
}

pub fn login_page_html_with_google(google_enabled: bool) -> String {
    render_page(LOGIN_HTML)
        .replace(
            "<button id=\"passkey-login\" class=\"secondary\" type=\"button\">パスキーでログイン</button>",
            "<button id=\"passkey-login\" class=\"secondary\" type=\"button\">パスキーでログイン</button>__GOOGLE_LOGIN_BUTTON__",
        )
        .replace(
            "const form=document.querySelector('#login-form')",
            "const googleError=new URLSearchParams(location.search).get('error');if(googleError==='google')document.querySelector('#message').textContent='Googleアカウントでのログインに失敗しました。';if(googleError==='google_inactive')document.querySelector('#message').textContent='このユーザーは無効化されています。';const form=document.querySelector('#login-form')",
        )
        .replace(
            "__GOOGLE_LOGIN_BUTTON__",
            if google_enabled {
                "<a id=\"google-login\" href=\"/auth/google\" style=\"display:flex;align-items:center;justify-content:center;width:100%;height:44px;margin-top:10px;border:1px solid #cbd5e1;border-radius:8px;color:#1f2937;text-decoration:none;font-weight:700;font-size:14px;background:#fff\">Googleアカウントでログイン</a>"
            } else {
                ""
            },
        )
}

pub fn detail_page(project_id: i64, role: &str) -> String {
    render_page(DETAIL_HTML)
        .replace("__PROJECT_ID__", &project_id.to_string())
        .replace("__CURRENT_USER_ROLE__", role)
}

pub async fn detail_page_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    headers: HeaderMap,
) -> Result<Html<String>, ApiError> {
    let user = authenticate_request(&state, &headers).await?;
    Ok(Html(detail_page(id, &user.1)))
}

pub async fn admin_page(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Html<String>, ApiError> {
    require_management_page(&state, &headers, false).await?;
    Ok(Html(render_page(ADMIN_HTML)))
}

pub async fn admin_projects_page(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Html<String>, ApiError> {
    require_management_page(&state, &headers, false).await?;
    Ok(Html(render_page(ADMIN_PROJECTS_HTML)))
}

pub async fn user_registration_page(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Html<String>, ApiError> {
    let user = authenticate_request(&state, &headers).await?;
    if !matches!(user.1.as_str(), "admin" | "viewer") {
        return Err(ApiError::Forbidden);
    }
    Ok(Html(render_page(USER_REGISTRATION_HTML)))
}

pub async fn dealer_registration_page(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Html<String>, ApiError> {
    require_management_page(&state, &headers, false).await?;
    Ok(Html(render_page(DEALER_REGISTRATION_HTML)))
}

async fn require_management_page(
    state: &AppState,
    headers: &HeaderMap,
    admin_only: bool,
) -> Result<(), ApiError> {
    let user = authenticate_request(state, headers).await?;
    if (admin_only && user.1 != "admin") || (!admin_only && !can_manage_projects(&user.1)) {
        return Err(ApiError::Forbidden);
    }
    Ok(())
}

pub async fn login_page() -> Html<String> {
    Html(login_page_html())
}

pub async fn help_page() -> Html<String> {
    Html(render_page(HELP_HTML))
}
