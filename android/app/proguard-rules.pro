# Keep rules for the release build (R8).
#
# Both dependencies ship their own consumer rules, so this file is deliberately
# almost empty: OkHttp documents how it reflects at `okhttp3.internal.platform`
# to pick a TLS provider, and kotlinx-serialization keeps its generated
# serializers, in `META-INF/proguard/*.pro` inside their own artifacts. Adding
# speculative `-keep` rules on top would undo the shrinking they are designed to
# cooperate with.
#
# What is NOT automatic is anything reached only from the platform by name, and
# this app has exactly one of those: the activity the launcher starts. AGP emits
# that rule for the manifest, so there is nothing to add here.
#
# The rules below are therefore only for diagnostics, so a release stack trace
# can still be read back.
-keepattributes SourceFile,LineNumberTable
-renamesourcefileattribute SourceFile
