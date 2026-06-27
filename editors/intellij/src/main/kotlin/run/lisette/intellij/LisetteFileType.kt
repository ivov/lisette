package run.lisette.intellij

import com.intellij.openapi.fileTypes.LanguageFileType
import com.intellij.openapi.util.IconLoader
import org.jetbrains.plugins.textmate.TextMateBackedFileType
import javax.swing.Icon

object LisetteFileType : LanguageFileType(LisetteLanguage), TextMateBackedFileType {
    override fun getName(): String = "Lisette"
    override fun getDescription(): String = "Lisette source file"
    override fun getDefaultExtension(): String = "lis"
    override fun getIcon(): Icon = IconLoader.getIcon("/icons/lisette.svg", LisetteFileType::class.java)
}
