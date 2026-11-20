plugins { `java-library`; `maven-publish`; kotlin("jvm") version "2.2.0" }
group = "dev.grafikr"
version = "1.1.0"
repositories { mavenCentral() }
dependencies {
    api("com.google.code.gson:gson:2.13.2")
    testImplementation("org.junit.jupiter:junit-jupiter:5.12.2")
    testRuntimeOnly("org.junit.platform:junit-platform-launcher:1.12.2")
}
java { toolchain { languageVersion.set(JavaLanguageVersion.of(21)) } }
kotlin { jvmToolchain(21) }
tasks.test {
    useJUnitPlatform()
    environment("GRAFIKR_FIXTURE_EARLY_SUBSCRIBE", "1")
}

val runtimeJar = tasks.register<Jar>("runtimeJar") {
    val bundle = providers.gradleProperty("grafikrRuntimeBundle")
    val classifier = providers.gradleProperty("grafikrRuntimeClassifier")
    if (bundle.orNull.isNullOrBlank() || classifier.orNull.isNullOrBlank()) {
        throw GradleException("grafikrRuntimeBundle and grafikrRuntimeClassifier are required")
    }
    archiveBaseName.set("grafikr-runtime")
    archiveClassifier.set(classifier)
    from(bundle.map { file(it) }) {
        into("runtime")
    }
}

publishing {
    publications {
        create<MavenPublication>("grafikr") {
            from(components["java"])
            artifactId = "grafikr-jvm"
        }
        create<MavenPublication>("runtime") {
            groupId = project.group.toString()
            artifactId = "grafikr-runtime"
            version = project.version.toString()
            artifact(runtimeJar)
        }
    }
    repositories {
        maven {
            name = "LocalRepository"
            url = uri(providers.gradleProperty("mavenRepoPath").orElse(layout.buildDirectory.dir("maven-repository").get().asFile.absolutePath).get())
        }
    }
}
