// Keel's Android register (ADR-0027): the kernel for Android, the register's screens and their
// state, and the app.

pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "keel-android"

// The register's state and what it asks of the kernel, in Kotlin alone, tested on the JVM.
include(":register")
// The kernel for Android: its native libraries, the bindings, and the register's adapter to them.
include(":kernel")
// The app: its screens, keys and services.
include(":app")
