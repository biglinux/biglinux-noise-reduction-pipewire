"""GTK4 building blocks shared by the BigLinux Python programs."""


def install():
    """Turn on what every program gets without changing a call site: the
    tooltip card, the closing-dialog focus fix and the popup-parent release.
    Call once, in the application's ``startup``."""
    from big_gtk_kit import dialog, surface, tooltip

    tooltip.install()
    dialog.install()
    surface.install()
