//! The zone name ICU reports as the machine default
//! (`Intl.DateTimeFormat().resolvedOptions().timeZone`) for an IANA name.
//! Bun on macOS uses the system ICU, which keeps legacy names
//! (`Asia/Calcutta`); elsewhere it bundles a newer ICU (`Asia/Kolkata`).
//! Measured with Bun over every name in the tz database; names absent here
//! report as given.

/// macOS system ICU names, sorted by IANA name for binary search.
const MACOS_ALIASES: &[(&str, &str)] = &[
    ("Africa/Asmara", "Africa/Asmera"),
    ("Africa/Timbuktu", "Africa/Bamako"),
    ("America/Argentina/Buenos_Aires", "America/Buenos_Aires"),
    ("America/Argentina/Catamarca", "America/Catamarca"),
    ("America/Argentina/ComodRivadavia", "America/Catamarca"),
    ("America/Argentina/Cordoba", "America/Cordoba"),
    ("America/Argentina/Jujuy", "America/Jujuy"),
    ("America/Argentina/Mendoza", "America/Mendoza"),
    ("America/Atikokan", "America/Coral_Harbour"),
    ("America/Atka", "America/Adak"),
    ("America/Ensenada", "America/Tijuana"),
    ("America/Fort_Wayne", "America/Indianapolis"),
    ("America/Indiana/Indianapolis", "America/Indianapolis"),
    ("America/Kentucky/Louisville", "America/Louisville"),
    ("America/Knox_IN", "America/Indiana/Knox"),
    ("America/Montreal", "America/Toronto"),
    ("America/Nipigon", "America/Toronto"),
    ("America/Nuuk", "America/Godthab"),
    ("America/Pangnirtung", "America/Iqaluit"),
    ("America/Porto_Acre", "America/Rio_Branco"),
    ("America/Rainy_River", "America/Winnipeg"),
    ("America/Rosario", "America/Cordoba"),
    ("America/Santa_Isabel", "America/Tijuana"),
    ("America/Shiprock", "America/Denver"),
    ("America/Thunder_Bay", "America/Toronto"),
    ("America/Virgin", "America/St_Thomas"),
    ("America/Yellowknife", "America/Edmonton"),
    ("Antarctica/South_Pole", "Antarctica/McMurdo"),
    ("Asia/Ashkhabad", "Asia/Ashgabat"),
    ("Asia/Choibalsan", "Asia/Ulaanbaatar"),
    ("Asia/Chongqing", "Asia/Shanghai"),
    ("Asia/Chungking", "Asia/Shanghai"),
    ("Asia/Dacca", "Asia/Dhaka"),
    ("Asia/Harbin", "Asia/Shanghai"),
    ("Asia/Ho_Chi_Minh", "Asia/Saigon"),
    ("Asia/Istanbul", "Europe/Istanbul"),
    ("Asia/Kashgar", "Asia/Urumqi"),
    ("Asia/Kathmandu", "Asia/Katmandu"),
    ("Asia/Kolkata", "Asia/Calcutta"),
    ("Asia/Macao", "Asia/Macau"),
    ("Asia/Tel_Aviv", "Asia/Jerusalem"),
    ("Asia/Thimbu", "Asia/Thimphu"),
    ("Asia/Ujung_Pandang", "Asia/Makassar"),
    ("Asia/Ulan_Bator", "Asia/Ulaanbaatar"),
    ("Asia/Yangon", "Asia/Rangoon"),
    ("Atlantic/Faroe", "Atlantic/Faeroe"),
    ("Atlantic/Jan_Mayen", "Arctic/Longyearbyen"),
    ("Australia/ACT", "Australia/Sydney"),
    ("Australia/Canberra", "Australia/Sydney"),
    ("Australia/Currie", "Australia/Hobart"),
    ("Australia/LHI", "Australia/Lord_Howe"),
    ("Australia/NSW", "Australia/Sydney"),
    ("Australia/North", "Australia/Darwin"),
    ("Australia/Queensland", "Australia/Brisbane"),
    ("Australia/South", "Australia/Adelaide"),
    ("Australia/Tasmania", "Australia/Hobart"),
    ("Australia/Victoria", "Australia/Melbourne"),
    ("Australia/West", "Australia/Perth"),
    ("Australia/Yancowinna", "Australia/Broken_Hill"),
    ("Brazil/Acre", "America/Rio_Branco"),
    ("Brazil/DeNoronha", "America/Noronha"),
    ("Brazil/East", "America/Sao_Paulo"),
    ("Brazil/West", "America/Manaus"),
    ("CET", "Europe/Brussels"),
    ("CST6CDT", "UTC"),
    ("Canada/Atlantic", "America/Halifax"),
    ("Canada/Central", "America/Winnipeg"),
    ("Canada/Eastern", "America/Toronto"),
    ("Canada/Mountain", "America/Edmonton"),
    ("Canada/Newfoundland", "America/St_Johns"),
    ("Canada/Pacific", "America/Vancouver"),
    ("Canada/Saskatchewan", "America/Regina"),
    ("Canada/Yukon", "America/Whitehorse"),
    ("Chile/Continental", "America/Santiago"),
    ("Chile/EasterIsland", "Pacific/Easter"),
    ("Cuba", "America/Havana"),
    ("EET", "Europe/Athens"),
    ("EST", "America/Panama"),
    ("EST5EDT", "UTC"),
    ("Egypt", "Africa/Cairo"),
    ("Eire", "Europe/Dublin"),
    ("Etc/GMT", "UTC"),
    ("Etc/GMT+0", "UTC"),
    ("Etc/GMT-0", "UTC"),
    ("Etc/GMT0", "UTC"),
    ("Etc/Greenwich", "UTC"),
    ("Etc/UCT", "UTC"),
    ("Etc/UTC", "UTC"),
    ("Etc/Universal", "UTC"),
    ("Etc/Zulu", "UTC"),
    ("Europe/Belfast", "Europe/London"),
    ("Europe/Kyiv", "Europe/Kiev"),
    ("Europe/Nicosia", "Asia/Nicosia"),
    ("Europe/Tiraspol", "Europe/Chisinau"),
    ("Europe/Uzhgorod", "Europe/Kiev"),
    ("Europe/Zaporozhye", "Europe/Kiev"),
    ("GB", "Europe/London"),
    ("GB-Eire", "Europe/London"),
    ("GMT", "UTC"),
    ("GMT+0", "UTC"),
    ("GMT-0", "UTC"),
    ("GMT0", "UTC"),
    ("Greenwich", "UTC"),
    ("HST", "Pacific/Honolulu"),
    ("Hongkong", "Asia/Hong_Kong"),
    ("Iceland", "Atlantic/Reykjavik"),
    ("Iran", "Asia/Tehran"),
    ("Israel", "Asia/Jerusalem"),
    ("Jamaica", "America/Jamaica"),
    ("Japan", "Asia/Tokyo"),
    ("Kwajalein", "Pacific/Kwajalein"),
    ("Libya", "Africa/Tripoli"),
    ("MET", "Europe/Brussels"),
    ("MST", "America/Phoenix"),
    ("MST7MDT", "UTC"),
    ("Mexico/BajaNorte", "America/Tijuana"),
    ("Mexico/BajaSur", "America/Mazatlan"),
    ("Mexico/General", "America/Mexico_City"),
    ("NZ", "Pacific/Auckland"),
    ("NZ-CHAT", "Pacific/Chatham"),
    ("Navajo", "America/Denver"),
    ("PRC", "Asia/Shanghai"),
    ("PST8PDT", "UTC"),
    ("Pacific/Chuuk", "Pacific/Truk"),
    ("Pacific/Johnston", "Pacific/Honolulu"),
    ("Pacific/Kanton", "Pacific/Enderbury"),
    ("Pacific/Pohnpei", "Pacific/Ponape"),
    ("Pacific/Samoa", "Pacific/Pago_Pago"),
    ("Pacific/Yap", "Pacific/Truk"),
    ("Poland", "Europe/Warsaw"),
    ("Portugal", "Europe/Lisbon"),
    ("ROC", "Asia/Taipei"),
    ("ROK", "Asia/Seoul"),
    ("Singapore", "Asia/Singapore"),
    ("Turkey", "Europe/Istanbul"),
    ("UCT", "UTC"),
    ("US/Alaska", "America/Anchorage"),
    ("US/Aleutian", "America/Adak"),
    ("US/Arizona", "America/Phoenix"),
    ("US/Central", "America/Chicago"),
    ("US/East-Indiana", "America/Indianapolis"),
    ("US/Eastern", "America/New_York"),
    ("US/Hawaii", "Pacific/Honolulu"),
    ("US/Indiana-Starke", "America/Indiana/Knox"),
    ("US/Michigan", "America/Detroit"),
    ("US/Mountain", "America/Denver"),
    ("US/Pacific", "America/Los_Angeles"),
    ("US/Samoa", "Pacific/Pago_Pago"),
    ("Universal", "UTC"),
    ("W-SU", "Europe/Moscow"),
    ("WET", "Europe/Lisbon"),
    ("Zulu", "UTC"),
];

const ALIASES: &[(&str, &str)] = if cfg!(target_os = "macos") {
    MACOS_ALIASES
} else {
    super::icu_zones_bundled::ALIASES
};

/// The ICU name for `name`, or `name` itself.
pub fn icu_zone_name(name: &str) -> &str {
    ALIASES
        .binary_search_by(|(k, _)| (*k).cmp(name))
        .map_or(name, |i| ALIASES[i].1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_are_sorted_and_unique() {
        for table in [MACOS_ALIASES, super::super::icu_zones_bundled::ALIASES] {
            assert!(table.windows(2).all(|w| w[0].0 < w[1].0));
        }
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn maps_legacy_names_to_bundled_icu_names() {
        for (iana, icu) in [
            ("Asia/Calcutta", "Asia/Kolkata"),
            ("US/Pacific", "America/Los_Angeles"),
            ("Etc/UTC", "UTC"),
            ("Europe/Kiev", "Europe/Kyiv"),
            ("EST", "America/Panama"),
            ("Asia/Kolkata", "Asia/Kolkata"),
            ("Europe/Berlin", "Europe/Berlin"),
        ] {
            assert_eq!(icu_zone_name(iana), icu, "{iana}");
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn maps_backward_links_to_icu_names() {
        for (iana, icu) in [
            ("Asia/Kolkata", "Asia/Calcutta"),
            ("US/Pacific", "America/Los_Angeles"),
            ("Etc/UTC", "UTC"),
            ("GMT", "UTC"),
            ("Europe/Kyiv", "Europe/Kiev"),
            ("Asia/Ho_Chi_Minh", "Asia/Saigon"),
            ("Asia/Kathmandu", "Asia/Katmandu"),
            ("Europe/Berlin", "Europe/Berlin"),
            ("Asia/Calcutta", "Asia/Calcutta"),
        ] {
            assert_eq!(icu_zone_name(iana), icu, "{iana}");
        }
    }
}
