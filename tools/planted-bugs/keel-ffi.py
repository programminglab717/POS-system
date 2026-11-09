"""Planted bugs for keel-ffi: see run.py.

The crate's tests are known answers, in its own unit tests; the Kotlin tests in bindings/kotlin
drive the same calls from the JVM. Every bug here is a unit test's to catch.
"""

BUGS = [
    (
        "ffi: a choice's parent is ignored",
        "src/register.rs",
        "            Some((_, parent)) => parent.choices.insert(0, choice),",
        "            Some((_, _)) => roots.push(choice),",
        "unit",
    ),
    (
        "ffi: a choice may be its own parent",
        "src/register.rs",
        "let parent = usize::try_from(parent).ok().filter(|&parent| parent < at);",
        "let parent = usize::try_from(parent).ok().filter(|&parent| parent <= at);",
        "unit",
    ),
    (
        "ffi: what is due crosses without its text",
        "src/register.rs",
        "            RuntimeError::TenderShort { due } => KeelError::TenderShort {",
        "            RuntimeError::TenderShort { due } if due.is_zero() => KeelError::TenderShort {",
        "unit",
    ),
    (
        "ffi: groups under a modifier are as deep as the item's",
        "src/records.rs",
        "            flatten(&modifier.groups, Some(&id), depth.saturating_add(1), out);",
        "            flatten(&modifier.groups, Some(&id), depth, out);",
        "unit",
    ),
    (
        "ffi: a modifier left out crosses as an extra",
        "src/records.rs",
        "            order::Prefix::No => Prefix::No,",
        "            order::Prefix::No => Prefix::Extra,",
        "unit",
    ),
    (
        "ffi: a printer that closed reads as no word",
        "src/records.rs",
        "                    keel_print::Unconfirmed::Closed => Unconfirmed::Closed,",
        "                    keel_print::Unconfirmed::Closed => Unconfirmed::NoWord,",
        "unit",
    ),
]
