pub mod home;
pub mod detail;
pub mod history;
pub mod settings;
pub mod cloud_saves;

#[cfg(test)]
mod home_test;
#[cfg(test)]
mod cloud_saves_test;

pub use home::*;
pub use detail::*;
pub use history::*;
pub use settings::*;
pub use cloud_saves::*;