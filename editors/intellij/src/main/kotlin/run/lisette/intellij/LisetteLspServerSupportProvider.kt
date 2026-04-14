package run.lisette.intellij

import com.intellij.execution.configurations.GeneralCommandLine
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.platform.lsp.api.LspServerSupportProvider
import com.intellij.platform.lsp.api.ProjectWideLspServerDescriptor
import com.intellij.platform.lsp.api.customization.LspFormattingSupport

internal class LisetteLspServerSupportProvider : LspServerSupportProvider {
    override fun fileOpened(
        project: Project,
        file: VirtualFile,
        serverStarter: LspServerSupportProvider.LspServerStarter,
    ) {
        if (isLisetteFile(file)) {
            serverStarter.ensureServerStarted(LisetteLspServerDescriptor(project))
        }
    }
}

private class LisetteLspServerDescriptor(project: Project) :
    ProjectWideLspServerDescriptor(project, "Lisette") {

    override fun isSupportedFile(file: VirtualFile): Boolean = isLisetteFile(file)

    override fun createCommandLine(): GeneralCommandLine =
        GeneralCommandLine("lis", "lsp")

    override val lspFormattingSupport: LspFormattingSupport = object : LspFormattingSupport() {
        override fun shouldFormatThisFileExclusivelyByServer(
            file: VirtualFile,
            ideCanFormatThisFileItself: Boolean,
            serverExplicitlyWantsToFormatThisFile: Boolean,
        ): Boolean = true
    }
}

private fun isLisetteFile(file: VirtualFile): Boolean =
    file.name.endsWith(".lis")

