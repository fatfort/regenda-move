mod loading_scene;
mod day_scene;
mod month_scene;
mod event_scene;
mod settings_scene;
mod oauth_scene;
mod weekly_scene;
mod keyboard_scene;
mod datetime_spin_scene;
mod edit_event_scene;

pub use loading_scene::LoadingScene;
pub use day_scene::DayScene;
pub use month_scene::MonthScene;
pub use event_scene::EventScene;
pub use settings_scene::SettingsScene;
pub use oauth_scene::OAuthScene;
pub use weekly_scene::WeeklyScene;
pub use keyboard_scene::{Keyboard, KeyboardOutcome};
pub use datetime_spin_scene::DateTimeSpin;
pub use edit_event_scene::{EditEventScene, EditMode, SaveMode};

use crate::canvas::Canvas;
use crate::rmpp_hal::types::InputEvent;
use downcast_rs::Downcast;

pub trait Scene: Downcast {
    fn on_input(&mut self, _event: InputEvent) {}
    fn draw(&mut self, canvas: &mut Canvas);
}
impl_downcast!(Scene);
