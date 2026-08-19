use time::{PrimitiveDateTime, UtcOffset};

pub const CHINA_OFFSET: UtcOffset = time::macros::offset!(+8);

pub fn china_now() -> PrimitiveDateTime {
    let now = time::OffsetDateTime::now_utc().to_offset(CHINA_OFFSET);
    PrimitiveDateTime::new(now.date(), now.time())
}

pub fn format_china(value: PrimitiveDateTime) -> Result<String, time::error::Format> {
    value
        .assume_offset(CHINA_OFFSET)
        .format(&time::format_description::well_known::Rfc3339)
}

#[cfg(test)]
mod tests {
    use time::{Date, Month, Time};

    use super::format_china;

    #[test]
    fn database_time_serializes_with_china_offset() {
        let value = Date::from_calendar_date(2026, Month::August, 18)
            .unwrap()
            .with_time(Time::from_hms(23, 59, 1).unwrap());
        assert_eq!(format_china(value).unwrap(), "2026-08-18T23:59:01+08:00");
    }
}
