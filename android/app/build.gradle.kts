// MeeGone's one APK (ADR 0005): a Java activity that hands Android's lifecycle to the Rust app,
// built from the Cargo workspace by cargo-ndk.

import javax.inject.Inject
import org.gradle.process.ExecOperations

plugins {
    id("com.android.application")
}

android {
    namespace = "io.github.twonet.meegone"
    compileSdk = 36
    ndkVersion = "27.3.13750724"
    // The host is plain Java on the Android SDK: no Kotlin, no AndroidX.
    enableKotlin = false

    defaultConfig {
        applicationId = "io.github.twonet.meegone"
        minSdk = 29 // Android 10, the first with the home-app role dialog (SRS 2.5).
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0"
        ndk {
            abiFilters += "arm64-v8a"
        }
    }

    buildTypes {
        debug {
            // The pseudo-locales en-XA (accented, a third longer) and ar-XB (right to left), so
            // the accessibility audit can check every screen in them (L10N-02, ADR 0040).
            isPseudoLocalesEnabled = true
        }
        // What tools/perf-bench.sh measures (ADR 0041): the release build, which a debuggable
        // build is far slower than, with the debug build's hooks for the probe and the
        // benchmark's 300 apps. It installs beside MeeGone, so a benchmark never touches its
        // layout or settings, and it is never released (SEC-06).
        create("benchmark") {
            initWith(getByName("release"))
            applicationIdSuffix = ".benchmark"
            signingConfig = signingConfigs.getByName("debug")
            isDebuggable = false
            // So simpleperf and Perfetto can profile it from adb.
            isProfileable = true
            matchingFallbacks += "release"
        }
    }

    sourceSets {
        getByName("benchmark") {
            java.directories += "src/debug/java"
        }
    }

    // A call newer than Android 10 without its version check fails the release build (COMP-01,
    // ADR 0044): assembleRelease runs lint's vital checks, which leave NewApi out by default.
    // They check the shipped code only; the debug hooks meet Android 10 on its emulator run.
    lint {
        fatal += "NewApi"
    }
}

/** Builds the Rust app as `libmeegone_launcher.so` for each ABI, with cargo-ndk. */
abstract class CargoNdk @Inject constructor(private val exec: ExecOperations) : DefaultTask() {
    @get:Internal
    abstract val ndk: DirectoryProperty

    @get:Internal
    abstract val workspace: DirectoryProperty

    @get:OutputDirectory
    abstract val jniLibs: DirectoryProperty

    /**
     * The Rust app's features: `faults` in the debug build alone, for tools/fault-check.sh
     * (ADR 0043). tools/arch-check checks it stays there, and that the release APK has none.
     */
    @get:Input
    abstract val features: ListProperty<String>

    @TaskAction
    fun build() {
        exec.exec {
            workingDir = workspace.get().asFile
            environment("ANDROID_NDK_HOME", ndk.get().asFile.path)
            commandLine(
                listOf(
                    "cargo", "ndk", "-t", "arm64-v8a", "-P", "29",
                    "-o", jniLibs.get().asFile.path,
                    "build", "--release", "-p", "meegone-launcher",
                ) + features.get().flatMap { listOf("--features", it) }
            )
        }
    }
}

androidComponents.onVariants { variant ->
    val cargo = tasks.register<CargoNdk>("cargoNdk" + variant.name.replaceFirstChar { it.uppercase() }) {
        ndk = androidComponents.sdkComponents.ndkDirectory
        workspace = rootProject.layout.projectDirectory.dir("..")
        features = if (variant.buildType == "debug") listOf("meegone-launcher/faults") else listOf()
        // Cargo knows best whether the library is up to date.
        outputs.upToDateWhen { false }
    }
    variant.sources.jniLibs?.addGeneratedSourceDirectory(cargo, CargoNdk::jniLibs)
}
