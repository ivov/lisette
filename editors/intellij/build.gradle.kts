plugins {
    kotlin("jvm") version "1.9.25"
    id("org.jetbrains.intellij.platform") version "2.14.0"
}

group = "run.lisette"
version = "0.1.0"

repositories {
    mavenCentral()
    intellijPlatform {
        defaultRepositories()
    }
}

dependencies {
    intellijPlatform {
        intellijIdeaUltimate("2024.3.5")
        bundledPlugin("org.jetbrains.plugins.textmate")
        pluginVerifier()
        zipSigner()
    }
}

val copyTmLanguageGrammar = tasks.register<Copy>("copyTmLanguageGrammar") {
    description = "Vendors the VSCode TextMate grammar into the plugin bundle directory."
    from("${rootDir}/../vscode/syntaxes/lisette.tmLanguage.json")
    into(layout.buildDirectory.dir("generated/textmate/textmate/bundles/lisette"))
}

sourceSets {
    main {
        resources {
            srcDir(layout.buildDirectory.dir("generated/textmate"))
        }
    }
}

tasks.processResources {
    dependsOn(copyTmLanguageGrammar)
}

kotlin {
    jvmToolchain(17)
}

intellijPlatform {
    pluginConfiguration {
        name = "Lisette"
        ideaVersion {
            sinceBuild = "243"
            untilBuild = provider { null }
        }
    }

    pluginVerification {
        ides {
            recommended()
        }
    }
}
