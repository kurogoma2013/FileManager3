use crate::handlers::auth_handlers::authenticate_request;
use crate::models::{ApiError, AppState};
use axum::{
    extract::{Path, State},
    http::HeaderMap,
    response::Html,
};
use std::sync::Arc;

struct Specification {
    slug: &'static str,
    title: &'static str,
    body: &'static str,
}

const SPECIFICATIONS: &[Specification] = &[
    Specification {
        slug: "00_maintenance",
        title: "保守ガイド",
        body: include_str!("../docs/00_maintenance.md"),
    },
    Specification {
        slug: "01_overview",
        title: "システム概要",
        body: include_str!("../docs/01_overview.md"),
    },
    Specification {
        slug: "02_permissions",
        title: "権限仕様",
        body: include_str!("../docs/02_permissions.md"),
    },
    Specification {
        slug: "03_screens",
        title: "画面仕様",
        body: include_str!("../docs/03_screens.md"),
    },
    Specification {
        slug: "04_api",
        title: "API仕様",
        body: include_str!("../docs/04_api.md"),
    },
    Specification {
        slug: "05_architecture_and_data",
        title: "アーキテクチャ・データ構造",
        body: include_str!("../docs/05_architecture_and_data.md"),
    },
    Specification {
        slug: "06_configuration",
        title: "設定・起動",
        body: include_str!("../docs/06_configuration.md"),
    },
    Specification {
        slug: "07_android",
        title: "Android版",
        body: include_str!("../docs/07_android.md"),
    },
    Specification {
        slug: "08_ios",
        title: "iOS版",
        body: include_str!("../docs/08_ios.md"),
    },
    Specification {
        slug: "09_ubuntu_letsencrypt",
        title: "Ubuntu・Let's Encrypt 設定",
        body: include_str!("../docs/09_ubuntu_letsencrypt.md"),
    },
    Specification {
        slug: "10_docker_postgresql",
        title: "Docker PostgreSQL",
        body: include_str!("../docs/10_docker_postgresql.md"),
    },
    Specification {
        slug: "11_ubuntu_apache",
        title: "Ubuntu・Apache 設定",
        body: include_str!("../docs/11_ubuntu_apache.md"),
    },
];

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn render_inline(value: &str) -> String {
    let escaped = escape_html(value);
    let mut output = String::new();
    let mut rest = escaped.as_str();
    while let Some(start) = rest.find('`') {
        output.push_str(&rest[..start]);
        let after_start = &rest[start + 1..];
        let Some(end) = after_start.find('`') else {
            output.push('`');
            output.push_str(after_start);
            break;
        };
        output.push_str("<code>");
        output.push_str(&after_start[..end]);
        output.push_str("</code>");
        rest = &after_start[end + 1..];
    }
    if !rest.is_empty() && !rest.contains('`') {
        output.push_str(rest);
    }
    replace_marked(&output, "**", "strong")
}

fn replace_marked(value: &str, marker: &str, tag: &str) -> String {
    let mut output = String::new();
    let mut rest = value;
    while let Some(start) = rest.find(marker) {
        output.push_str(&rest[..start]);
        let after_start = &rest[start + marker.len()..];
        let Some(end) = after_start.find(marker) else {
            output.push_str(marker);
            output.push_str(after_start);
            break;
        };
        output.push_str(&format!("<{tag}>{}</{tag}>", &after_start[..end]));
        rest = &after_start[end + marker.len()..];
    }
    if !rest.is_empty() && !rest.contains(marker) {
        output.push_str(rest);
    }
    output
}

fn table_cells(line: &str) -> Vec<String> {
    line.trim()
        .trim_matches('|')
        .split('|')
        .map(|cell| render_inline(cell.trim()))
        .collect()
}

fn is_table_separator(line: &str) -> bool {
    let trimmed = line.trim().trim_matches('|');
    !trimmed.is_empty()
        && trimmed.split('|').all(|cell| {
            let cell = cell.trim();
            cell.len() >= 3 && cell.chars().all(|ch| ch == '-' || ch == ':' || ch == ' ')
        })
}

fn render_table(lines: &[&str]) -> String {
    let mut html = String::from("<table><thead><tr>");
    for cell in table_cells(lines[0]) {
        html.push_str("<th>");
        html.push_str(&cell);
        html.push_str("</th>");
    }
    html.push_str("</tr></thead><tbody>");
    for line in &lines[2..] {
        html.push_str("<tr>");
        for cell in table_cells(line) {
            html.push_str("<td>");
            html.push_str(&cell);
            html.push_str("</td>");
        }
        html.push_str("</tr>");
    }
    html.push_str("</tbody></table>");
    html
}

fn render_markdown(markdown: &str) -> String {
    let lines: Vec<&str> = markdown.lines().collect();
    let mut html = String::new();
    let mut paragraph = Vec::new();
    let mut list: Option<&str> = None;
    let mut code = false;

    let flush_paragraph = |html: &mut String, paragraph: &mut Vec<&str>| {
        if !paragraph.is_empty() {
            html.push_str("<p>");
            html.push_str(
                &paragraph
                    .iter()
                    .map(|line| render_inline(line.trim()))
                    .collect::<Vec<_>>()
                    .join("<br>"),
            );
            html.push_str("</p>");
            paragraph.clear();
        }
    };
    let close_list = |html: &mut String, list: &mut Option<&str>| {
        if let Some(kind) = list.take() {
            html.push_str(&format!("</{kind}>"));
        }
    };

    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        if line.trim_start().starts_with("```") {
            flush_paragraph(&mut html, &mut paragraph);
            close_list(&mut html, &mut list);
            if code {
                html.push_str("</code></pre>");
            } else {
                html.push_str("<pre><code>");
            }
            code = !code;
            index += 1;
            continue;
        }
        if code {
            html.push_str(&escape_html(line));
            html.push('\n');
            index += 1;
            continue;
        }
        if line.trim().is_empty() {
            flush_paragraph(&mut html, &mut paragraph);
            close_list(&mut html, &mut list);
            index += 1;
            continue;
        }
        if index + 1 < lines.len()
            && line.trim().starts_with('|')
            && is_table_separator(lines[index + 1])
        {
            flush_paragraph(&mut html, &mut paragraph);
            close_list(&mut html, &mut list);
            let start = index;
            index += 2;
            while index < lines.len() && lines[index].trim().starts_with('|') {
                index += 1;
            }
            html.push_str(&render_table(&lines[start..index]));
            continue;
        }
        let trimmed = line.trim_start();
        if let Some((level, text)) = trimmed.split_once(' ') {
            if level.chars().all(|ch| ch == '#') && (1..=6).contains(&level.len()) {
                flush_paragraph(&mut html, &mut paragraph);
                close_list(&mut html, &mut list);
                html.push_str(&format!(
                    "<h{0}>{1}</h{0}>",
                    level.len(),
                    render_inline(text.trim())
                ));
                index += 1;
                continue;
            }
        }
        if let Some(item) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            flush_paragraph(&mut html, &mut paragraph);
            if list != Some("ul") {
                close_list(&mut html, &mut list);
                html.push_str("<ul>");
                list = Some("ul");
            }
            html.push_str("<li>");
            html.push_str(&render_inline(item.trim()));
            html.push_str("</li>");
            index += 1;
            continue;
        }
        if trimmed.len() > 3
            && trimmed.as_bytes()[0].is_ascii_digit()
            && trimmed[1..].starts_with(". ")
        {
            flush_paragraph(&mut html, &mut paragraph);
            if list != Some("ol") {
                close_list(&mut html, &mut list);
                html.push_str("<ol>");
                list = Some("ol");
            }
            html.push_str("<li>");
            html.push_str(&render_inline(&trimmed[3..]));
            html.push_str("</li>");
            index += 1;
            continue;
        }
        close_list(&mut html, &mut list);
        paragraph.push(line);
        index += 1;
    }
    flush_paragraph(&mut html, &mut paragraph);
    close_list(&mut html, &mut list);
    if code {
        html.push_str("</code></pre>");
    }
    html
}

async fn require_admin(state: &AppState, headers: &HeaderMap) -> Result<(), ApiError> {
    let (_, role) = authenticate_request(state, headers).await?;
    if role != "admin" {
        return Err(ApiError::Forbidden);
    }
    Ok(())
}

pub async fn index_page(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Html<String>, ApiError> {
    require_admin(&state, &headers).await?;

    let items = SPECIFICATIONS
        .iter()
        .map(|spec| {
            format!(
                "<a class=\"spec-card\" href=\"/admin/specifications/{}\"><strong>{}</strong><span>仕様書を表示する</span><span class=\"spec-arrow\" aria-hidden=\"true\">→</span></a>",
                spec.slug,
                escape_html(spec.title),
            )
        })
        .collect::<String>();

    let content = format!(
        "<div class=\"card\" style=\"max-width:1080px\"><h1 style=\"font-size:24px;margin:0 0 6px\">仕様書</h1><p style=\"color:var(--muted);font-size:13px;margin:0 0 18px\">FileManager3の仕様・設計ドキュメントです。管理者のみ閲覧できます。</p><div class=\"spec-grid\">{items}</div></div>"
    );

    Ok(Html(render_spec_page("仕様書", "", &content)))
}

pub async fn detail_page(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
    headers: HeaderMap,
) -> Result<Html<String>, ApiError> {
    require_admin(&state, &headers).await?;
    let spec = SPECIFICATIONS
        .iter()
        .find(|item| item.slug == slug)
        .ok_or(ApiError::NotFound)?;

    let content = format!(
        "<div class=\"card\" style=\"max-width:1080px\"><div style=\"display:flex;justify-content:space-between;align-items:center;margin-bottom:12px\"><h1 style=\"font-size:24px;margin:0\">{}</h1><a href=\"/admin/specifications\" class=\"btn secondary btn-sm\" style=\"text-decoration:none;display:inline-flex;align-items:center;gap:4px\">← 一覧へ戻る</a></div><article class=\"spec-document markdown-body\">{}</article></div>",
        escape_html(spec.title),
        render_markdown(spec.body),
    );

    let breadcrumb_sub = format!(" / <strong>{}</strong>", escape_html(spec.title));
    Ok(Html(render_spec_page(
        spec.title,
        &breadcrumb_sub,
        &content,
    )))
}

fn render_spec_page(title: &str, breadcrumb_sub: &str, content_html: &str) -> String {
    let spec_css = r#"<style>.spec-grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(250px,1fr));gap:14px;margin-top:16px}.spec-card{position:relative;display:flex;flex-direction:column;gap:8px;padding:20px 42px 20px 20px;background:#fff;border:1px solid var(--line);border-radius:12px;color:inherit;text-decoration:none;box-shadow:0 4px 12px rgba(18,35,61,.04);transition:all .15s ease}.spec-card:hover{border-color:var(--blue);transform:translateY(-1px);box-shadow:0 8px 24px rgba(18,35,61,.08)}.spec-card strong{font-size:15px;color:var(--navy)}.spec-card span{font-size:12px;color:var(--muted)}.spec-arrow{position:absolute;right:18px;top:50%;font-size:20px!important;color:var(--blue)!important;transform:translateY(-50%)}.spec-document{background:#fff;border:1px solid var(--line);border-radius:12px;box-shadow:0 4px 12px rgba(18,35,61,.04);overflow:auto;margin-top:16px}.markdown-body{padding:24px 28px;color:#263449;font-size:14px;line-height:1.8}.markdown-body h1,.markdown-body h2,.markdown-body h3,.markdown-body h4,.markdown-body h5,.markdown-body h6{color:var(--navy);line-height:1.35;margin:1.6em 0 .55em}.markdown-body h1{font-size:22px;border-bottom:1px solid var(--line);padding-bottom:10px}.markdown-body h2{font-size:18px;border-bottom:1px solid #edf1f6;padding-bottom:7px}.markdown-body h3{font-size:15px}.markdown-body p{margin:0 0 1em}.markdown-body ul,.markdown-body ol{margin:0 0 1em;padding-left:1.6em}.markdown-body li{margin:.25em 0}.markdown-body code{padding:2px 5px;background:#eef3f9;border-radius:5px;font:13px ui-monospace,SFMono-Regular,Menlo,monospace;color:#27456d}.markdown-body pre{margin:1em 0;padding:16px;overflow:auto;background:#172b46;color:#e6eef8;border-radius:8px}.markdown-body pre code{padding:0;background:transparent;color:inherit}.markdown-body table{width:100%;margin:1em 0;border-collapse:collapse;font-size:13px}.markdown-body th,.markdown-body td{padding:10px 12px;border:1px solid #dfe6ef;text-align:left;vertical-align:top}.markdown-body th{background:#f1f5f9;color:var(--navy);font-weight:700}.markdown-body a{color:var(--blue)}.markdown-body strong{color:var(--navy)}</style></head>"#;

    let target_card = r#"<div class="card"><h1 style="font-size:24px;margin:0 0 6px">管理メニュー</h1><p style="color:var(--muted);font-size:13px;margin:0 0 18px">案件・販売店・ユーザーなどの登録および設定管理を行います。</p><a class="menu" href="/admin/projects"><strong>案件管理</strong><span>案件の新規登録・一覧確認・情報の編集を行います。</span></a><a class="menu" href="/admin/dealers"><strong>販売店管理</strong><span>販売店の新規登録・一覧確認・名称および住所の編集を行います。</span></a><a class="menu admin-only-nav" href="/admin/specifications"><strong>仕様書</strong><span>FileManager3の仕様・権限・画面・API設計を確認します。</span></a><a class="menu admin-only-nav" href="/admin/users"><strong>ユーザー管理</strong><span>FileManagerを利用するユーザーの一覧確認・新規登録・設定編集を行います。</span></a></div>"#;

    let html = crate::templates::ADMIN_HTML
        .replace(
            "<title>管理メニュー | FileManager3</title>",
            &format!("<title>{} | FileManager3</title>", escape_html(title)),
        )
        .replace("</head>", spec_css)
        .replace(
            "<div class=\"breadcrumb\"><strong>管理メニュー</strong></div>",
            &format!(
                "<div class=\"breadcrumb\"><strong><a href=\"/admin\" style=\"text-decoration:none;color:inherit\">管理メニュー</a></strong> / <strong><a href=\"/admin/specifications\" style=\"text-decoration:none;color:inherit\">仕様書</a></strong>{}</div>",
                breadcrumb_sub
            ),
        )
        .replace(target_card, content_html);

    crate::templates::render_page(&html)
}

#[cfg(test)]
mod tests {
    use super::render_markdown;

    #[test]
    fn markdownを安全に整形して主要なブロックを描画する() {
        let html = render_markdown(
            "# 見出し\n\n- 項目\n- **重要**\n\n| A | B |\n| --- | --- |\n| <危険> | `code` |",
        );
        assert!(html.contains("<h1>見出し</h1>"));
        assert!(html.contains("<ul>"));
        assert!(html.contains("<strong>重要</strong>"));
        assert!(html.contains("<table>"));
        assert!(html.contains("&lt;危険&gt;"));
        assert!(html.contains("<code>code</code>"));
        assert!(!html.contains("<危険>"));
    }
}
