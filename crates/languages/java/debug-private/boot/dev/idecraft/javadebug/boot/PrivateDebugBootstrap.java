// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0
package dev.idecraft.javadebug.boot;

import java.io.IOException;
import java.io.InputStream;
import java.util.List;

import org.eclipse.core.runtime.IProgressMonitor;
import org.eclipse.jdt.ls.core.internal.IDelegateCommandHandler;
import org.osgi.framework.Bundle;
import org.osgi.framework.FrameworkUtil;
import org.osgi.framework.wiring.BundleWiring;

/**
 * The command jdtls knows. java-debug exports no packages, so the real handler, kept under
 * {@code impl/} in this bundle, is defined in a loader that delegates to java-debug's own.
 */
public final class PrivateDebugBootstrap implements IDelegateCommandHandler {
    private static final String HOST = "com.microsoft.java.debug.plugin";
    private static final String HANDLER = "dev.idecraft.javadebug.PrivateDebugCommand";

    private IDelegateCommandHandler handler;

    @Override
    public synchronized Object executeCommand(String command, List<Object> arguments, IProgressMonitor monitor)
            throws Exception {
        if (handler == null) {
            handler = (IDelegateCommandHandler) new Inner(hostLoader())
                    .loadClass(HANDLER).getDeclaredConstructor().newInstance();
        }
        return handler.executeCommand(command, arguments, monitor);
    }

    private static ClassLoader hostLoader() throws Exception {
        for (Bundle bundle : FrameworkUtil.getBundle(PrivateDebugBootstrap.class).getBundleContext().getBundles()) {
            if (HOST.equals(bundle.getSymbolicName())) {
                bundle.start(Bundle.START_TRANSIENT);
                return bundle.adapt(BundleWiring.class).getClassLoader();
            }
        }
        throw new IllegalStateException(HOST + " is not installed");
    }

    /** Defines the classes under {@code impl/}; everything else comes from java-debug's loader. */
    private static final class Inner extends ClassLoader {
        Inner(ClassLoader host) {
            super(host);
        }

        @Override
        protected Class<?> findClass(String name) throws ClassNotFoundException {
            String path = "impl/" + name.replace('.', '/') + ".class";
            try (InputStream in = PrivateDebugBootstrap.class.getClassLoader().getResourceAsStream(path)) {
                if (in == null) {
                    throw new ClassNotFoundException(name);
                }
                byte[] bytes = in.readAllBytes();
                return defineClass(name, bytes, 0, bytes.length);
            } catch (IOException e) {
                throw new ClassNotFoundException(name, e);
            }
        }
    }
}
