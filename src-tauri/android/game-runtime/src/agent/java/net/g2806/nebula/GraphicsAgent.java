package net.g2806.nebula;

import java.lang.instrument.ClassFileTransformer;
import java.lang.instrument.Instrumentation;
import java.security.ProtectionDomain;
import javassist.ClassPool;
import javassist.CtClass;
import javassist.CtMethod;
import javassist.LoaderClassPath;

/** SDL and LWJGL must resolve OpenGL functions through the same provider.
 * Android's EGL wrappers can otherwise give SDL a different function pointer
 * from the one used by LWJGL. RenderPearl correctly rejects that combination.
 * Keep its checks intact and fix the binding, rather than skipping validation.
 */
public final class GraphicsAgent implements ClassFileTransformer {
    public static void premain(String options, Instrumentation instrumentation) {
        instrumentation.addTransformer(new GraphicsAgent());
    }

    @Override public byte[] transform(ClassLoader loader, String name, Class<?> type,
                                     ProtectionDomain domain, byte[] bytes) {
        if (!"org/lwjgl/sdl/SDLVideo".equals(name)) return null;
        CtClass target = null;
        try {
            ClassPool pool = new ClassPool(false);
            pool.appendSystemPath();
            pool.insertClassPath(new LoaderClassPath(loader));
            target = pool.makeClass(new java.io.ByteArrayInputStream(bytes));
            CtMethod method = target.getDeclaredMethod("nSDL_GL_GetProcAddress");
            if (!"(J)J".equals(method.getSignature())) throw new IllegalStateException("Unsupported SDL OpenGL binding");
            method.setBody("{ if ($1 == 0L) return 0L; return org.lwjgl.opengl.GL.getFunctionProvider().getFunctionAddress(org.lwjgl.system.MemoryUtil.memASCII($1)); }");
            System.out.println("[Nebula] SDL OpenGL bindings use the LWJGL graphics provider");
            return target.toBytecode();
        } catch (Throwable error) {
            // Original bindings remain in place; RenderPearl still fails closed.
            System.err.println("[Nebula] Cannot align SDL OpenGL bindings: " + error.getClass().getSimpleName());
            return null;
        } finally {
            if (target != null) target.detach();
        }
    }
}
