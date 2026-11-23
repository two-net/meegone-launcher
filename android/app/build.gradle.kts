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

    @TaskAction
    fun build() {
        exec.exec {
            workingDir = workspace.get().asFile
            environment("ANDROID_NDK_HOME", ndk.get().asFile.path)
            commandLine(
                "cargo", "ndk", "-t", "arm64-v8a", "-P", "29",
                "-o", jniLibs.get().asFile.path,
                "build", "--release", "-p", "meegone-launcher",
            )
        }
    }
}

androidComponents.onVariants { variant ->
    val cargo = tasks.register<CargoNdk>("cargoNdk" + variant.name.replaceFirstChar { it.uppercase() }) {
        ndk = androidComponents.sdkComponents.ndkDirectory
        workspace = rootProject.layout.projectDirectory.dir("..")
        // Cargo knows best whether the library is up to date.
        outputs.upToDateWhen { false }
    }
    variant.sources.jniLibs?.addGeneratedSourceDirectory(cargo, CargoNdk::jniLibs)
}
