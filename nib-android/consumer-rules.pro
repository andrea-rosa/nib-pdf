-keep class com.sun.jna.** { *; }
-keep class * implements com.sun.jna.** { *; }
-keep class uniffi.nib_ffi.** { *; }
-dontwarn java.awt.**

# kotlinx.serialization: keep generated serializers + companions so
# @Serializable classes survive R8 in a minified consumer.
-keepclasseswithmembers class **$Companion {
    kotlinx.serialization.KSerializer serializer(...);
}
-keepclassmembers class * {
    @kotlinx.serialization.Serializable <init>(...);
}
-keepclasseswithmembers class **$$serializer {
    *** INSTANCE;
}
-keep,includedescriptorclasses class io.github.nibpdf.nib.**$$serializer { *; }
-keepclassmembers class io.github.nibpdf.nib.** { *** Companion; }
-keepclasseswithmembers class io.github.nibpdf.nib.** { kotlinx.serialization.KSerializer serializer(...); }
-keepclassmembers class io.github.nibpdf.nib.** { kotlinx.serialization.KSerializer serializer(...); }
