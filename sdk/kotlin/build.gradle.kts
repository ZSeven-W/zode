import org.gradle.api.publish.maven.MavenPublication

plugins {
    kotlin("jvm") version "2.0.21"
    kotlin("plugin.serialization") version "2.0.21"
    `maven-publish`
}

group = "com.zseven.zode"
version = "0.2.0-beta.4"

publishing {
    publications {
        create<MavenPublication>("github") {
            from(components["java"])
            artifactId = "zode-sdk"
            pom {
                name.set("Zode Kotlin SDK")
                description.set("Kotlin/JVM SDK for the zode app-server JSON-RPC protocol.")
                url.set("https://github.com/ZSeven-W/zode")
                licenses {
                    license {
                        name.set("MIT")
                        url.set("https://github.com/ZSeven-W/zode/blob/main/LICENSE")
                    }
                }
                scm {
                    url.set("https://github.com/ZSeven-W/zode")
                    connection.set("scm:git:https://github.com/ZSeven-W/zode.git")
                }
            }
        }
    }
    repositories {
        maven {
            name = "GitHubPackages"
            url = uri("https://maven.pkg.github.com/zseven-w/zode")
            credentials {
                username = System.getenv("GITHUB_ACTOR")
                password = System.getenv("GITHUB_TOKEN")
            }
        }
    }
}

dependencies {
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.7.3")
    testImplementation(kotlin("test"))
    // Assumptions.assumeTrue is used to skip the opt-in ZODE_BIN e2e test.
    testImplementation("org.junit.jupiter:junit-jupiter-api:5.10.2")
}

tasks.test {
    useJUnitPlatform()
}
