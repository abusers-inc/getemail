use crate::Filter;

pub mod logical {
    use crate::Filter;

    pub struct AndFilter<First, Second> {
        first_filter: First,
        second_filter: Second,
    }

    impl<F, S> AndFilter<F, S> {
        pub fn new(f: F, s: S) -> Self {
            Self {
                first_filter: f,
                second_filter: s,
            }
        }
    }

    impl<First: Filter, Second: Filter> Filter for AndFilter<First, Second> {
        fn filter(&self, msg: &crate::OwnedMessage) -> bool {
            self.first_filter.filter(msg) && self.second_filter.filter(msg)
        }
    }
}
pub mod subject {
    use crate::Filter;

    pub struct SubjectFilter {
        subject: String,
    }
    impl SubjectFilter {
        pub fn new(subject: String) -> Self {
            Self { subject }
        }
    }

    impl Filter for SubjectFilter {
        fn filter(&self, msg: &crate::OwnedMessage) -> bool {
            let Some(msg_subject) = msg.borrow_dependent().subject() else {
                return false;
            };

            msg_subject == self.subject.as_str()
        }
    }
}
pub mod date {
    use chrono::{DateTime, Utc};

    use crate::Filter;

    pub enum DateFilterMode {
        Since,
        Earlier,
    }
    pub struct DateFilter {
        mode: DateFilterMode,
        date: DateTime<Utc>,
    }

    impl DateFilter {
        pub fn since(date: DateTime<Utc>) -> Self {
            Self {
                mode: DateFilterMode::Since,
                date,
            }
        }

        pub fn since_now() -> Self {
            Self {
                mode: DateFilterMode::Since,
                date: Utc::now(),
            }
        }
    }

    impl Filter for DateFilter {
        fn filter(&self, msg: &crate::OwnedMessage) -> bool {
            let Some(msg_date) = msg
                .borrow_dependent()
                .date()
                .and_then(|x| DateTime::<Utc>::from_timestamp(x.to_timestamp(), 0))
            else {
                return false;
            };
            match self.mode {
                DateFilterMode::Since => msg_date > self.date,
                DateFilterMode::Earlier => msg_date < self.date,
            }
        }
    }
}
#[cfg(feature = "regex")]
pub mod regex {
    use std::borrow::Cow;

    use crate::{Filter, OwnedMessage};

    pub struct RegexFilter {
        regex: regex::Regex,
    }

    impl RegexFilter {
        pub fn new(regex: regex::Regex) -> Self {
            Self { regex }
        }
    }

    impl Filter for RegexFilter {
        fn filter(&self, msg: &OwnedMessage) -> bool {
            let msg = msg.borrow_dependent();
            let empty = Cow::Owned(String::new());
            let body = msg.body_html(0).unwrap_or(empty);

            self.regex.is_match(body.as_ref())
        }
    }
}

pub struct NoFilter;
impl super::Filter for NoFilter {
    fn filter(&self, msg: &crate::OwnedMessage) -> bool {
        true
    }
}

impl<T: Filter> Filter for &T {
    fn filter(&self, msg: &crate::OwnedMessage) -> bool {
        T::filter(self, msg)
    }
}
