pub fn can_manage_projects(role: &str) -> bool {
    matches!(role, "admin" | "member")
}

pub fn can_upload_files(role: &str) -> bool {
    matches!(role, "admin" | "member" | "viewer")
}

pub fn can_move_file(role: &str, file_type: &str) -> bool {
    matches!(role, "admin" | "member") || (role == "viewer" && file_type == "Picture")
}

pub fn can_delete_permanently(role: &str) -> bool {
    role == "admin"
}

/// 管理ユーザーを削除できるのは管理者だけに限定する。
pub fn can_delete_user_target(requester_role: &str, target_role: &str) -> bool {
    target_role != "admin" || requester_role == "admin"
}

/// 管理者は全ユーザー、viewerは自分自身だけを編集できる。
pub fn can_edit_user(requester_id: i64, requester_role: &str, target_id: i64) -> bool {
    requester_role == "admin" || (requester_role == "viewer" && requester_id == target_id)
}
