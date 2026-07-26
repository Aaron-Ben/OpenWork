//! 库中所有时间字段的统一口径。
//!
//! 时间列一律是 `TIMESTAMP WITHOUT TIME ZONE`，存东八区墙上时间。naive 列本身不携带
//! 时区，数据库无法替我们检查口径，所以落库的时间值只能来自本模块——任何绕过它的写入
//! 都会造成静默的 8 小时偏差。见 `.claude/rules/database.md`。

use time::format_description::well_known::Rfc3339;
use time::{OffsetDateTime, PrimitiveDateTime, UtcOffset};

/// 东八区。
pub(crate) const CHINA_OFFSET: UtcOffset = match UtcOffset::from_hms(8, 0, 0) {
    Ok(offset) => offset,
    Err(_) => panic!("+08:00 is a valid offset"),
};

/// 当前的东八区墙上时间。
///
/// 目前落库的时间要么来自列默认值 / SQL 的 `CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'`，
/// 要么来自 [`to_china`] 转换的既有瞬间，因此还没有调用点。保留它是为了让"Rust 侧需要
/// 当前时间"时有唯一合法入口，而不是让人临时写 `now_utc() + 8h`。
#[allow(dead_code)]
pub(crate) fn china_now() -> PrimitiveDateTime {
    to_china(OffsetDateTime::now_utc())
}

/// 把一个瞬间转成东八区墙上时间。外部来源（Provider 响应、Trace Guard 采集的时刻）
/// 都是瞬间，落库前必须经过这里。
pub(crate) fn to_china(value: OffsetDateTime) -> PrimitiveDateTime {
    let local = value.to_offset(CHINA_OFFSET);
    PrimitiveDateTime::new(local.date(), local.time())
}

/// 把库里的东八区墙上时间序列化成带 `+08:00` 的 RFC 3339 字符串。
///
/// 偏移量必须如实标注：库里存的是东八区，若标成 `Z`，前端会在已经是东八区的值上
/// 再做一次换算，最终偏 16 小时且全程不报错。
#[allow(dead_code)]
pub(crate) fn to_wire(value: PrimitiveDateTime) -> Option<String> {
    value.assume_offset(CHINA_OFFSET).format(&Rfc3339).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_an_instant_to_beijing_wall_clock() {
        let instant = OffsetDateTime::parse("2026-07-25T10:30:45Z", &Rfc3339).unwrap();

        let stored = to_china(instant);

        assert_eq!(stored.to_string(), "2026-07-25 18:30:45.0");
    }

    #[test]
    fn serializes_stored_values_with_an_east_eight_offset() {
        let instant = OffsetDateTime::parse("2026-07-25T10:30:45Z", &Rfc3339).unwrap();

        let wire = to_wire(to_china(instant)).expect("formattable");

        assert!(wire.starts_with("2026-07-25T18:30:45"), "wire = {wire}");
        assert!(wire.ends_with("+08:00"), "wire = {wire}");
    }

    #[test]
    fn a_round_trip_through_storage_preserves_the_instant() {
        let instant = OffsetDateTime::parse("2026-07-25T10:30:45Z", &Rfc3339).unwrap();

        let wire = to_wire(to_china(instant)).expect("formattable");
        let parsed = OffsetDateTime::parse(&wire, &Rfc3339).unwrap();

        assert_eq!(parsed, instant);
    }
}
