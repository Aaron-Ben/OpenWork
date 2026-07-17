/// UTF-8 安全地把输出截断到 `max_bytes` 预算内，采用 head+tail 策略：
/// 保留开头 3/4 + 结尾 1/4，中间用省略标记记录隐藏行数。
pub(crate) fn truncate_output(s: String, max_bytes: usize) -> String {
    if s.len() <= max_bytes || max_bytes == 0 {
        return s;
    }
    let marker_budget = 64.min(max_bytes);
    let budget = max_bytes - marker_budget;
    let head_budget = budget * 3 / 4;
    let tail_budget = budget - head_budget;

    let head_end = floor_char_boundary(&s, head_budget);
    let tail_start = floor_char_boundary(&s, s.len().saturating_sub(tail_budget));

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
        let input: String = (0..1000).map(|i| format!("{i}\n")).collect();
        let out = truncate_output(input, 200);
        assert!(out.contains("行已隐藏"), "must contain hidden marker");
        assert!(out.starts_with("0\n"), "head must be preserved");
        assert!(out.contains("999"), "tail must be preserved");
    }

    #[test]
    fn truncate_is_utf8_safe_on_multibyte() {
        let input: String = "中".repeat(2000);
        let out = truncate_output(input, 300);
        assert!(out.chars().count() > 0);
        let _ = out.len();
    }
}
