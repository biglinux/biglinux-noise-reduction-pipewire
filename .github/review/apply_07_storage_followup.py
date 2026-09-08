from pathlib import Path
from _common import done, replace, commit

TITLE = 'fix(settings): validate migrated snapshots without rereading or following save symlinks'
if not done(TITLE):
    path = 'src/config.rs'
    text = Path(path).read_text()
    start = text.index('    pub fn load_strict()')
    end = text.index('    /// Settings may persist an optional model', start)
    old = text[start:end]
    migration_start = old.index('            // Older standalone settings')
    migration_end = old.index('            serde_json::from_value::<Self>(value)', migration_start)
    migration = old[migration_start:migration_end]
    text = text[:start] + '''    /// Strict transaction read: migration, validation and normalization use
    /// one byte snapshot. Malformed files are never converted to defaults.
    pub fn load_strict() -> io::Result<Self> {
        Self::load_from_strict(&settings_file())
    }

    pub fn load_from_strict(path: &Path) -> io::Result<Self> {
        let content = match read_to_string(path) {
            Ok(content) => content,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(error),
        };
        let mut value: serde_json::Value = serde_json::from_str(&content)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        if !value.is_object() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "settings must be a JSON object"));
        }
''' + migration + '''        let mut settings: Self = serde_json::from_value(value)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        settings.equalizer.normalize();
        settings.output_filter.equalizer.normalize();
        settings.window = settings.window.sanitized();
        settings.demote_unavailable_models();
        Ok(settings)
    }

    #[must_use]
    pub fn load() -> Self {
        Self::load_from(&settings_file())
    }

    /// Read-only compatibility entry point. Writers use load_strict instead.
    pub fn load_from(path: &Path) -> Self {
        Self::load_from_strict(path).unwrap_or_else(|error| {
            error!("settings: could not read {}: {error}", path.display());
            Self::default()
        })
    }

''' + text[end:]
    text = text.replace('use log::{debug, error, info};', 'use log::{debug, error};')
    text = text.replace('if std::fs::read(path).is_ok_and(|existing| existing == json) {', 'if !path.is_symlink() && std::fs::read(path).is_ok_and(|existing| existing == json) {')
    Path(path).write_text(text)
    path = 'src/config/storage.rs'
    text = Path(path).read_text()
    old = 'let mut stored = match std::fs::read(path) {'
    assert text.count(old) == 1
    text = text.replace(old, '''// REPLACE_DESTINATION replaces the link itself. Never read or modify
    // the unrelated file a settings symlink may point at during that write.
    let existing = if path.is_symlink() {
        Err(io::Error::from(io::ErrorKind::NotFound))
    } else {
        std::fs::read(path)
    };
    let mut stored = match existing {''')
    Path(path).write_text(text)
    commit(TITLE, ['src/config.rs', path])

TITLE = 'fix(accessibility): honor reduced motion without freezing meter values'
if not done(TITLE):
    path = 'src/ui/widgets/spectrum.rs'
    text = Path(path).read_text()
    old = '''        if !animate {
            self.area.queue_draw();
            return;
        }'''
    if old in text:
        text = text.replace(old, '''        if !animate {
            let mut state = self.state.borrow_mut();
            state.bands = state.target_bands;
            state.peaks = state.target_bands;
            state.peak_level = state.target_peak;
            state.peak_hold = state.target_peak;
            drop(state);
            self.area.queue_draw();
            return;
        }''')
    Path(path).write_text(text)
    # Add the regression even when the direct-snap branch was already fixed.
    with Path(path).open('a') as file:
        file.write('''
#[cfg(test)]
mod meter_value_tests {
    #[test]
    fn native_meter_values_stay_finite_and_inside_the_range() {
        assert_eq!(super::finite_db(f32::NAN), -60.0);
        assert_eq!(super::finite_db(f32::INFINITY), -60.0);
        assert_eq!(super::finite_db(-120.0), -60.0);
        assert_eq!(super::finite_db(20.0), 0.0);
        assert_eq!(super::finite_db(-18.0), -18.0);
    }
}
''')
    commit(TITLE, [path])
