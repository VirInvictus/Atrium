# Translation catalogue build. Regenerating the pot from the Rust
# sources needs xgettext >= 0.25 (native Rust extraction); building
# from the checked-in pot/po files has no such requirement.
i18n.gettext(gettext_package,
  preset: 'glib',
  args: [
    # The atrium/src/i18n.rs helper names. The glib preset's keywords
    # (_ / C_ / N_ / ...) stay active; these are additive.
    '--keyword=gettext',
    '--keyword=ngettext:1,2',
    '--keyword=pgettext:1c,2',
    '--keyword=gettext_f',
    '--keyword=ngettext_f:1,2',
  ],
)
