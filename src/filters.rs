use std::ops::Deref;

use chrono::{DateTime, Utc};
use date::DateFilter;
use logical::{And, Or};
use sender::Sender;
use subject::Subject;

use crate::OwnedMessage;

pub trait Filter: Send + Sync {
    fn filter(&self, msg: &OwnedMessage) -> bool;

    fn dynamize(self) -> Box<dyn Filter>
    where
        Self: Sized + 'static,
    {
        Box::new(self)
    }
}

impl<'a, T: Filter> Filter for &'a T {
    fn filter(&self, _: &OwnedMessage) -> bool {
        todo!()
    }
}

impl Filter for () {
    fn filter(&self, _: &OwnedMessage) -> bool {
        true
    }
}

impl Filter for Box<dyn Filter> {
    fn filter(&self, msg: &OwnedMessage) -> bool {
        self.deref().filter(msg)
    }
}

macro_rules! define_impl_ext {
    {
        impl $filters:ident {
            $(
                pub fn $name:ident($($arg_name:ident: $arg_type:ty),*) -> impl Filter $name_block:block
            )*
        }
    } => {
        impl $filters {
            $(
                pub fn $name($($arg_name: $arg_type),*) -> impl Filter $name_block
            )*
        }

        paste::paste! {
            pub trait FilterExt: Sized {
                fn and(self, second: impl Filter) -> impl Filter;
                fn or(self, second: impl Filter) -> impl Filter;

                $(
                    fn [<and_ $name>](self, $($arg_name: $arg_type),*) -> impl Filter;
                    fn [<or_ $name>](self, $($arg_name: $arg_type),*) -> impl Filter;
                )*
            }

            impl<T: Filter + Sized + 'static> FilterExt for T {
                    fn and(self, second: impl Filter) -> impl Filter {
                        And::new(self, second)
                    }
                    fn or(self, second: impl Filter) -> impl Filter {
                        Or::new(self, second)
                    }

                $(

                    fn [<and_ $name>](self, $($arg_name: $arg_type),*) -> impl Filter {
                        self.and($filters::$name($($arg_name),*))
                    }
                    fn [<or_ $name>](self, $($arg_name: $arg_type),*) -> impl Filter {
                        self.or($filters::$name($($arg_name),*))
                    }

                )*

            }

        }

    };
}

pub struct Filters;
impl Filters {
    pub fn empty() -> impl Filter {
        ()
    }
}

define_impl_ext! {
    impl Filters {
        pub fn subject(s: impl Into<String>) -> impl Filter {
            Subject::new(s.into())
        }

        pub fn sender(s: impl Into<String>) -> impl Filter {
            Sender::new(s.into())
        }

        pub fn sent_since(s: impl Into<DateTime<Utc>>) -> impl Filter {
            DateFilter::since(s.into())
        }
    }
}

mod logical {
    use super::Filter;

    pub struct And<First, Second> {
        first_filter: First,
        second_filter: Second,
    }

    impl<F, S> And<F, S> {
        pub fn new(f: F, s: S) -> Self {
            Self {
                first_filter: f,
                second_filter: s,
            }
        }
    }

    impl<First: Filter, Second: Filter> Filter for And<First, Second> {
        fn filter(&self, msg: &crate::OwnedMessage) -> bool {
            self.first_filter.filter(msg) && self.second_filter.filter(msg)
        }
    }

    pub struct Or<First, Second> {
        first_filter: First,
        second_filter: Second,
    }

    impl<F, S> Or<F, S> {
        pub fn new(f: F, s: S) -> Self {
            Self {
                first_filter: f,
                second_filter: s,
            }
        }
    }

    impl<First: Filter, Second: Filter> Filter for Or<First, Second> {
        fn filter(&self, msg: &crate::OwnedMessage) -> bool {
            self.first_filter.filter(msg) || self.second_filter.filter(msg)
        }
    }
}
mod subject {
    use super::Filter;

    pub struct Subject {
        subject: String,
    }
    impl Subject {
        pub fn new(subject: String) -> Self {
            Self { subject }
        }
    }

    impl Filter for Subject {
        fn filter(&self, msg: &crate::OwnedMessage) -> bool {
            let Some(msg_subject) = msg.subject() else {
                return false;
            };

            msg_subject == self.subject.as_str()
        }
    }
}

mod sender {
    use super::Filter;

    pub struct Sender {
        sender: String,
    }

    impl Sender {
        pub fn new(name: String) -> Self {
            Self { sender: name }
        }
    }

    impl Filter for Sender {
        fn filter(&self, msg: &crate::OwnedMessage) -> bool {
            msg.sender().is_some_and(|a| a.contains(&self.sender))
        }
    }
}

mod date {
    use chrono::{DateTime, Utc};

    use super::Filter;

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
