#![macro_use]
macro_rules! string_id_class {
    ( $name:ident ) => {
        #[derive(Clone, PartialEq, Eq, Hash, Debug)]
        pub struct $name(pub String);

        impl $name {
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(
                &self,
                formatter: &mut std::fmt::Formatter<'_>,
            ) -> std::result::Result<(), std::fmt::Error> {
                formatter.write_str(&self.0)
            }
        }

        impl From<&String> for $name {
            fn from(input: &String) -> $name {
                $name(input.clone())
            }
        }

        impl From<&str> for $name {
            fn from(input: &str) -> $name {
                $name(String::from(input))
            }
        }
    };
}
