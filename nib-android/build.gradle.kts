import org.gradle.api.tasks.Exec

plugins {
    alias(libs.plugins.android.library)
    alias(libs.plugins.kotlin.compose)
    alias(libs.plugins.kotlin.serialization)
    alias(libs.plugins.ktlint)
}

val nibFfiVersion: String by lazy {
    val toml = rootProject.file("nib-rs/Cargo.toml").readText()
    val pkg = toml.substringAfter("[workspace.package]")
    Regex("""version\s*=\s*"([^"]+)"""").find(pkg)?.groupValues?.get(1)
        ?: error("cannot parse workspace version from nib-rs/Cargo.toml")
}

fun cargoBin(): String {
    providers.environmentVariable("CARGO").orNull?.let { return it }
    val home = File(System.getProperty("user.home"), ".cargo/bin/cargo")
    return if (home.isFile) home.absolutePath else "cargo"
}

fun hostAbi(): String = if (System.getProperty("os.arch").startsWith("aarch64")) "arm64-v8a" else "x86_64"

fun abisFor(variantName: String): List<String> {
    val prop = providers.gradleProperty("nib.abis").orNull
    if (prop != null) return prop.split(",").map { it.trim() }.filter { it.isNotEmpty() }
    return if (variantName.equals("release", ignoreCase = true)) {
        listOf("arm64-v8a", "armeabi-v7a", "x86_64")
    } else {
        listOf(hostAbi())
    }
}

android {
    namespace = "io.github.nibpdf.nib"
    compileSdk = 37
    ndkVersion = "29.0.14206865"

    defaultConfig {
        minSdk = 26
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        consumerProguardFiles("consumer-rules.pro")
        buildConfigField("String", "NIB_FFI_VERSION", "\"$nibFfiVersion\"")
    }

    buildFeatures {
        buildConfig = true
        compose = true
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

kotlin {
    compilerOptions {
        jvmTarget = org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17
        explicitApi = org.jetbrains.kotlin.gradle.dsl.ExplicitApiMode.Warning
    }
}

val fetchPdfium =
    tasks.register<Exec>("fetchPdfium") {
        executable = "${rootProject.projectDir}/nib-rs/tools/fetch-pdfium.sh"
        args("--android-only")
        outputs.dir("src/main/jniLibs")
    }
tasks.named("preBuild") { dependsOn(fetchPdfium) }

val rustInputs =
    fileTree(rootProject.file("nib-rs")) {
        include("**/*.rs", "**/Cargo.toml", "Cargo.lock")
        exclude("target/**")
    }

androidComponents {
    onVariants { variant ->
        val cap = variant.name.replaceFirstChar { it.uppercase() }
        val rawDir = layout.buildDirectory.dir("rustJniLibs/${variant.name}/raw")
        val jniDir = layout.buildDirectory.dir("rustJniLibs/${variant.name}/jni")
        val genDir = layout.buildDirectory.dir("generated/uniffi/${variant.name}")
        val release = variant.name.equals("release", ignoreCase = true)

        val cargo =
            tasks.register<Exec>("cargoBuild$cap") {
                workingDir = rootProject.file("nib-rs")
                executable = cargoBin()
                args("ndk")
                abisFor(variant.name).forEach { args("-t", it) }
                args("-P", "26", "-o", rawDir.get().asFile.absolutePath, "build", "-p", "nib-ffi")
                if (release) args("--release")
                environment(
                    "ANDROID_NDK_HOME",
                    androidComponents.sdkComponents.ndkDirectory
                        .get()
                        .asFile.absolutePath,
                )
                inputs.files(rustInputs)
                outputs.dir(rawDir)
                dependsOn(fetchPdfium)
            }

        val jniLibs =
            tasks.register<Sync>("copyRustJniLibs$cap") {
                from(cargo.map { it.outputs.files }) {
                    include("**/libnib_ffi.so")
                }
                into(jniDir)
            }
        android.sourceSets
            .maybeCreate(variant.name)
            .jniLibs
            .directories
            .add("build/rustJniLibs/${variant.name}/jni")
        tasks
            .matching { it.name == "merge${cap}JniLibFolders" || it.name == "package${cap}JniLibs" }
            .configureEach { dependsOn(jniLibs) }

        val bindgenLib =
            tasks.register<Exec>("bindgenLib$cap") {
                // Run bindgen on the host debug libnib_ffi: the release profile
                // strips the ELF symbols the UniFFI metadata lives in, and the
                // generated Kotlin is identical across targets anyway.
                workingDir = rootProject.file("nib-rs")
                executable = cargoBin()
                args("build", "-p", "nib-ffi")
                inputs.files(rustInputs)
                outputs.file(
                    rootProject.file(
                        "nib-rs/target/debug/" +
                            (
                                if (System.getProperty("os.name").lowercase().contains("mac")) {
                                    "libnib_ffi.dylib"
                                } else {
                                    "libnib_ffi.so"
                                }
                            ),
                    ),
                )
            }
        val bindgen =
            tasks.register<Exec>("uniffiBindgen$cap") {
                workingDir = rootProject.file("nib-rs")
                executable = cargoBin()
                args(
                    "run",
                    "--quiet",
                    "-p",
                    "uniffi-bindgen",
                    "--",
                    "generate",
                    bindgenLib.map { it.outputs.files.singleFile.absolutePath }.get(),
                    "--language",
                    "kotlin",
                    "--out-dir",
                    genDir.get().asFile.absolutePath,
                    "--no-format",
                )
                inputs.files(rustInputs)
                outputs.dir(genDir)
                dependsOn(bindgenLib)
            }
        android.sourceSets
            .maybeCreate(variant.name)
            .kotlin
            .directories
            .add("build/generated/uniffi/${variant.name}")
        tasks
            .matching {
                (it.name.startsWith("compile$cap") && it.name.contains("Kotlin")) ||
                    it.name == "extract${cap}Annotations" ||
                    it.name.startsWith("runKtlintCheckOver${cap}SourceSet") ||
                    it.name.startsWith("runKtlintFormatOver${cap}SourceSet")
            }.configureEach { dependsOn(bindgen) }
    }
}

android {
    sourceSets {
        named("androidTest") {
            assets.directories.add("../nib-rs/nib-core/tests/fixtures")
        }
    }
}

dependencies {
    implementation(platform(libs.compose.bom))
    implementation(libs.compose.foundation)
    implementation(libs.compose.runtime)
    implementation(libs.compose.ui)
    implementation(libs.kotlinx.coroutines.android)
    implementation(libs.kotlinx.serialization.json)
    implementation(libs.androidx.collection.ktx)
    implementation(libs.jna) {
        artifact {
            type = "aar"
        }
    }
    androidTestImplementation(libs.compose.ui.test.junit4)
    androidTestImplementation(libs.activity.compose)
    androidTestImplementation(libs.androidx.test.ext.junit)
    androidTestImplementation(libs.androidx.test.runner)
    androidTestImplementation(libs.androidx.test.core)
    debugImplementation(libs.compose.ui.test.manifest)
    testImplementation(libs.junit)
    testImplementation(libs.kotlinx.coroutines.test)
}

ktlint {
    filter {
        exclude { it.file.path.contains("/build/") }
    }
}
