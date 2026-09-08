from pathlib import Path
from _common import done, replace, write, commit

TITLE = 'perf(spectrum): reuse FFT workspace and frame magnitude buffers'
if not done(TITLE):
    path = 'src/services/audio_monitor/analyzer.rs'
    replace(path, '    scratch: Vec<Complex32>,', '''    scratch: Vec<Complex32>,
    fft_workspace: Vec<Complex32>,
    magnitudes: Vec<f32>,
    normalization: f32,''')
    replace(path, '''        let band_boundaries = log_band_boundaries(&configuration);''', '''        let fft_workspace = vec![Complex32::new(0.0, 0.0); fft.get_inplace_scratch_len()];
        let magnitudes = vec![0.0; configuration.fft_size / 2];
        let normalization = 2.0 / window.iter().sum::<f32>();
        let band_boundaries = log_band_boundaries(&configuration);''')
    replace(path, '''            scratch,
            band_boundaries,''', '''            scratch,
            fft_workspace,
            magnitudes,
            normalization,
            band_boundaries,''')
    replace(path, '''        self.fft.process(&mut self.scratch);''', '''        self.fft.process_with_scratch(&mut self.scratch, &mut self.fft_workspace);''')
    replace(path, '''        let norm = 2.0 / self.window.iter().sum::<f32>();
        let half = self.configuration.fft_size / 2;
        let mut mag = vec![0.0_f32; half];
        for (i, m) in mag.iter_mut().enumerate() {
            *m = self.scratch[i].norm() * norm;
        }''', '''        let half = self.configuration.fft_size / 2;
        for (value, sample) in self.magnitudes.iter_mut().zip(&self.scratch) {
            *value = sample.norm() * self.normalization;
        }
        let mag = &self.magnitudes;''')
    replace(path, '''        let mut planner = FftPlanner::<f32>::new();''', '''        assert!(configuration.fft_size >= 4, "FFT window must contain at least four samples");
        assert!(configuration.sample_rate > 0, "sample rate must be nonzero");
        assert!(configuration.band_count > 0 && configuration.band_count <= configuration.fft_size / 2,
            "band count must fit the FFT bins");
        assert!(configuration.min_hz.is_finite() && configuration.max_hz.is_finite()
            && configuration.min_hz > 0.0 && configuration.max_hz > configuration.min_hz
            && configuration.min_hz < configuration.sample_rate as f32 / 2.0,
            "frequency range must be finite, ordered and below Nyquist");
        let mut planner = FftPlanner::<f32>::new();''')
    replace(path, '''    #[test]
    fn hann_window_endpoints_are_zero()''', '''    #[test]
    fn repeated_analysis_reuses_its_working_storage() {
        let mut analyzer = Analyzer::new(AnalyzerConfig::default());
        let input = vec![0.0; analyzer.configuration.fft_size];
        let buffers = (analyzer.scratch.as_ptr(), analyzer.fft_workspace.as_ptr(), analyzer.magnitudes.as_ptr());
        for _ in 0..100 {
            let frame = analyzer.analyze_samples(&input);
            assert!(frame.bands_db.iter().all(|value| value.is_finite()));
            assert_eq!(buffers, (analyzer.scratch.as_ptr(), analyzer.fft_workspace.as_ptr(), analyzer.magnitudes.as_ptr()));
        }
    }

    #[test]
    fn hann_window_endpoints_are_zero()''')
    capture = 'src/services/audio_monitor/capture.rs'
    text = Path(capture).read_text()
    start = text.index('    /// Copy the current ring buffer')
    end = text.index('\n}\n\nimpl Drop for Capture', start)
    text = text[:start] + '''    /// Copy into reusable caller-owned storage in chronological order.
    pub(super) fn copy_window_into(&self, out: &mut Vec<f32>) {
        out.clear();
        out.extend_from_slice(&self.ring[self.write_pos..]);
        out.extend_from_slice(&self.ring[..self.write_pos]);
    }
''' + text[end:]
    text = text.replace('[`Self::window_snapshot`]', '[`Self::copy_window_into`]')
    # PipeWire raw floats use native endianness, not always little endian.
    text = text.replace('f32::from_le_bytes', 'f32::from_ne_bytes')
    text = text.replace(' application.name=Filter noise', ' application.name=\\"Filter noise\\"')
    Path(capture).write_text(text)
    monitor = 'src/services/audio_monitor.rs'
    replace(monitor, '''    debug!("audio monitor: loop start");''', '''    let mut window = Vec::with_capacity(monitor_config.analyzer.fft_size);
    debug!("audio monitor: loop start");''')
    replace(monitor, '''        let window = capture.window_snapshot();''', '''        capture.copy_window_into(&mut window);''')
    commit(TITLE, [path, capture, monitor])

TITLE = 'feat(accessibility): expose live microphone values through a native GTK meter'
if not done(TITLE):
    path = 'src/ui/widgets/spectrum.rs'
    replace(path, '''    area: gtk::DrawingArea,''', '''    area: gtk::DrawingArea,
    root: gtk::Box,
    level: gtk::LevelBar,
    readout: gtk::Label,
    last_meter_update: Cell<Option<std::time::Instant>>,''')
    replace(path, '''        area.set_accessible_role(gtk::AccessibleRole::Meter);''', '''        area.set_accessible_role(gtk::AccessibleRole::Presentation);''')
    replace(path, '''        let state = Rc::new(RefCell::new(SpectrumState::default()));''', '''        let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let readout = gtk::Label::builder().xalign(0.0).wrap(true).build();
        readout.set_text(&i18n("Speak normally to check your microphone level."));
        let level = gtk::LevelBar::for_interval(-60.0, 0.0);
        level.set_value(-60.0);
        level.add_offset_value("low", -30.0);
        level.add_offset_value("high", -6.0);
        level.add_offset_value("full", -1.0);
        level.update_property(&[
            gtk::accessible::Property::Label(&i18n("Microphone level meter")),
            gtk::accessible::Property::Description(&i18n("Input level in decibels. Reduce microphone volume if it reaches zero.")),
        ]);
        root.append(&readout);
        root.append(&level);
        root.append(&area);
        let state = Rc::new(RefCell::new(SpectrumState::default()));''')
    replace(path, '''            area,
            state,''', '''            area,
            root,
            level,
            readout,
            last_meter_update: Cell::new(None),
            state,''')
    replace(path, '''        self.area.upcast_ref()''', '''        self.root.upcast_ref()''')
    replace(path, '''        self.state.borrow_mut().update_targets(frame);''', '''        self.state.borrow_mut().update_targets(frame);
        let now = std::time::Instant::now();
        if self.last_meter_update.get().is_none_or(|previous| now.duration_since(previous) >= Duration::from_millis(250)) {
            self.last_meter_update.set(Some(now));
            let rms = finite_db(frame.rms_db);
            let peak = finite_db(frame.peak_db);
            self.level.set_value(f64::from(rms));
            let text = i18n("Level: {level} dB · Peak: {peak} dB")
                .replace("{level}", &format!("{rms:.0}"))
                .replace("{peak}", &format!("{peak:.0}"));
            self.readout.set_text(&text);
        }''')
    replace(path, '''        if self.state.borrow_mut().advance_animation(true) {''', '''        let animate = gtk::Settings::default().is_none_or(|settings| settings.is_gtk_enable_animations());
        if !animate {
            self.area.queue_draw();
            return;
        }
        if self.state.borrow_mut().advance_animation(true) {''')
    replace(path, '''impl Drop for Spectrum {''', '''fn finite_db(value: f32) -> f32 {
    if value.is_finite() { value.clamp(-60.0, 0.0) } else { -60.0 }
}

impl Drop for Spectrum {''')
    commit(TITLE, [path])
