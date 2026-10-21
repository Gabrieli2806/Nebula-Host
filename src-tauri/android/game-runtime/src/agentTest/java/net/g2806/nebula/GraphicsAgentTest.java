package net.g2806.nebula;

import java.io.ByteArrayInputStream;
import java.net.URL;
import java.net.URLClassLoader;
import java.nio.file.Paths;
import java.util.Arrays;
import java.util.jar.JarFile;
import javassist.ClassPool;
import javassist.CtClass;
import javassist.CtMethod;
import javassist.LoaderClassPath;
import javassist.bytecode.CodeAttribute;
import javassist.bytecode.CodeIterator;
import javassist.bytecode.ConstPool;
import javassist.bytecode.InstructionPrinter;
import javassist.bytecode.Opcode;

/** Runs on the build machine, against the exact SDL binding shipped in the APK. */
public final class GraphicsAgentTest {
    public static void main(String[] args) throws Exception {
        java.io.File folder = Paths.get(args[0]).toFile();
        URL[] jars = Arrays.stream(folder.listFiles((dir, name) -> name.endsWith(".jar")))
            .map(file -> { try { return file.toURI().toURL(); } catch (Exception e) { throw new RuntimeException(e); } })
            .toArray(URL[]::new);
        try (URLClassLoader loader = new URLClassLoader(jars, GraphicsAgentTest.class.getClassLoader());
             JarFile jar = new JarFile(new java.io.File(folder, "lwjgl-sdl.jar"))) {
            byte[] original = jar.getInputStream(jar.getJarEntry("org/lwjgl/sdl/SDLVideo.class")).readAllBytes();
            GraphicsAgent agent = new GraphicsAgent();
            if (agent.transform(loader, "unrelated/Class", null, null, original) != null) throw new AssertionError("Unrelated class modified");
            byte[] patched = agent.transform(loader, "org/lwjgl/sdl/SDLVideo", null, null, original);
            if (patched == null) throw new AssertionError("Actual SDL binding could not be transformed");
            ClassPool beforePool = new ClassPool(true);
            ClassPool afterPool = new ClassPool(true);
            beforePool.insertClassPath(new LoaderClassPath(loader));
            afterPool.insertClassPath(new LoaderClassPath(loader));
            CtClass before = beforePool.makeClass(new ByteArrayInputStream(original));
            CtClass after = afterPool.makeClass(new ByteArrayInputStream(patched));
            for (CtMethod method : before.getDeclaredMethods()) {
                if (method.getName().equals("nSDL_GL_GetProcAddress")) continue;
                CodeAttribute a = method.getMethodInfo().getCodeAttribute();
                if (a != null && !instructions(method).equals(instructions(after.getDeclaredMethod(method.getName(), method.getParameterTypes())))) throw new AssertionError("Other SDL method changed: " + method.getName());
            }
            CtMethod binding = after.getDeclaredMethod("nSDL_GL_GetProcAddress");
            ConstPool constants = binding.getMethodInfo().getConstPool();
            CodeIterator code = binding.getMethodInfo().getCodeAttribute().iterator();
            boolean sharedProvider = false;
            while (code.hasNext()) {
                int position = code.next();
                if (code.byteAt(position) == Opcode.INVOKESTATIC) {
                    int ref = code.u16bitAt(position + 1);
                    if (constants.getMethodrefClassName(ref).equals("org.lwjgl.opengl.GL") && constants.getMethodrefName(ref).equals("getFunctionProvider")) sharedProvider = true;
                }
            }
            if (!sharedProvider) throw new AssertionError("SDL does not resolve through the shared GL provider");
            System.out.println("PASS: packaged SDL transforms; shared provider verified; other SDL methods unchanged");
        }
    }

    private static String instructions(CtMethod method) throws Exception {
        CodeIterator code = method.getMethodInfo().getCodeAttribute().iterator();
        StringBuilder text = new StringBuilder();
        while (code.hasNext()) {
            int position = code.next();
            // Serialization may renumber constant-pool entries. Compare the
            // instructions and their resolved symbols, rather than those IDs.
            text.append(InstructionPrinter.instructionString(code, position, method.getMethodInfo().getConstPool()).replaceAll("#\\d+", "#")).append('\n');
        }
        return text.toString();
    }
}
