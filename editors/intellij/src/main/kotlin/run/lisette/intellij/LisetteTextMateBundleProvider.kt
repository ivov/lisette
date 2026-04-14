package run.lisette.intellij

import org.jetbrains.plugins.textmate.api.TextMateBundleProvider
import org.jetbrains.plugins.textmate.api.TextMateBundleProvider.PluginBundle
import java.nio.file.Files
import java.nio.file.Path
import java.nio.file.StandardCopyOption

internal class LisetteTextMateBundleProvider : TextMateBundleProvider {
    override fun getBundles(): List<PluginBundle> =
        bundleDir?.let { listOf(PluginBundle("lisette", it)) } ?: emptyList()

    companion object {
        private val BUNDLE_FILES = listOf("package.json", "lisette.tmLanguage.json")
        private const val BUNDLE_RESOURCE_ROOT = "textmate/bundles/lisette"

        private val bundleDir: Path? by lazy { extractBundle() }

        private fun extractBundle(): Path? {
            val loader = LisetteTextMateBundleProvider::class.java.classLoader
            val dir = Files.createTempDirectory("lisette-tmbundle")
            dir.toFile().deleteOnExit()
            for (name in BUNDLE_FILES) {
                val stream = loader.getResourceAsStream("$BUNDLE_RESOURCE_ROOT/$name")
                    ?: return null
                stream.use { Files.copy(it, dir.resolve(name), StandardCopyOption.REPLACE_EXISTING) }
            }
            return dir
        }
    }
}
