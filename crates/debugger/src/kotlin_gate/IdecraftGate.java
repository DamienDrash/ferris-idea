import java.io.File;
import java.lang.instrument.ClassFileTransformer;
import java.lang.instrument.Instrumentation;
import java.security.ProtectionDomain;

/** Holds the program before its main class is defined until the debugger is ready. */
public final class IdecraftGate implements ClassFileTransformer {
    private final String mainClass;
    private final File flag;
    private volatile boolean open;

    private IdecraftGate(String mainClass, File flag) {
        this.mainClass = mainClass;
        this.flag = flag;
    }

    public static void premain(String arguments, Instrumentation instrumentation) {
        int cut = arguments == null ? -1 : arguments.indexOf('=');
        if (cut < 0) {
            return;
        }
        String mainClass = arguments.substring(0, cut).replace('.', '/');
        instrumentation.addTransformer(new IdecraftGate(mainClass, new File(arguments.substring(cut + 1))));
    }

    @Override
    public byte[] transform(ClassLoader loader, String name, Class<?> redefined,
                            ProtectionDomain domain, byte[] bytes) {
        if (!open && mainClass.equals(name)) {
            long end = System.nanoTime() + 30_000_000_000L;
            try {
                while (flag.exists() && System.nanoTime() < end) {
                    Thread.sleep(5);
                }
            } catch (InterruptedException e) {
                Thread.currentThread().interrupt();
            }
            open = true;
        }
        return null;
    }
}
