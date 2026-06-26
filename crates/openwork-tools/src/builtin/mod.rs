mod bash;
mod edit;
mod glob;
mod grep;
mod list;
mod read;
mod write;

use std::path::{Path, PathBuf};

pub use bash::Bash;
pub use edit::Edit;
pub use glob::Glob;
pub use grep::Grep;
pub use list::List;
pub use read::Read;
pub use write::Write;

/// 解析工具的 path 参数:绝对路径原样返回,相对路径基于工作目录解析。
pub(crate) fn resolve(working_dir: &Path, path: &str) -> PathBuf {
    let p = Path::new(path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        working_dir.join(p)
    }
}

/// UTF-8 安全地把输出截断到 `max_bytes` 预算内,采用 head+tail 策略:
/// 保留开头 3/4 + 结尾 1/4,中间用 "...N 行已隐藏..." 标记。
/// 替代 `String::truncate`(按字节截断,切在多字节字符中间会 panic)。
pub(crate) fn truncate_output(s: String, max_bytes: usize) -> String {
    if s.len() <= max_bytes || max_bytes == 0 {
        return s;
    }
    // 留一小段预算给省略号标记,剩余按 3:1 分给 head/tail。
    let marker_budget = 64.min(max_bytes);
    let budget = max_bytes - marker_budget;
    let head_budget = budget * 3 / 4;
    let tail_budget = budget - head_budget;

    let head_end = floor_char_boundary(&s, head_budget);
    let tail_start = floor_char_boundary(&s, s.len().saturating_sub(tail_budget));

    // head/tail 重叠(输入刚好略超预算)时退化为纯头部截断。
    if head_end >= tail_start {
        let cut = floor_char_boundary(&s, budget);
        let hidden = s[cut..].lines().count();
        let mut out = String::with_capacity(max_bytes);
        out.push_str(&s[..cut]);
        out.push_str(&format!("\n...[{} 行已隐藏]...", hidden));
        return out;
    }

    let hidden = s[head_end..tail_start].lines().count();
    let mut out = String::with_capacity(max_bytes);
    out.push_str(&s[..head_end]);
    out.push_str(&format!("\n...[{} 行已隐藏]...\n", hidden));
    out.push_str(&s[tail_start..]);
    out
}

/// 把索引回退到最近的 UTF-8 字符边界。
/// (`str::floor_char_boundary` 在当前工具链仍 unstable,手写等价实现。)
fn floor_char_boundary(s: &str, mut idx: usize) -> usize {
    if idx >= s.len() {
        return s.len();
    }
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_keeps_short_input_unchanged() {
        assert_eq!(truncate_output("hello".to_string(), 1024), "hello");
    }

    #[test]
    fn truncate_handles_empty() {
        assert_eq!(truncate_output(String::new(), 100), "");
    }

    #[test]
    fn truncate_head_tail_with_marker() {
        // 每行一个数字,远超预算;头部应含 "0"、尾部应含 "999"、中间有标记。
        let input: String = (0..1000).map(|i| format!("{i}\n")).collect();
        let out = truncate_output(input, 200);
        assert!(out.contains("行已隐藏"), "must contain hidden marker");
        assert!(out.starts_with("0\n"), "head must be preserved");
        assert!(out.contains("999"), "tail must be preserved");
    }

    #[test]
    fn truncate_is_utf8_safe_on_multibyte() {
        // 全是多字节中文字符,确保不 panic 且结果是合法 UTF-8(可正常迭代字符)。
        let input: String = "中".repeat(2000);
        let out = truncate_output(input, 300);
        assert!(out.chars().count() > 0);
        // 再次调用确保返回的是合法 &str 切片边界。
        let _ = out.len();
    }
}
