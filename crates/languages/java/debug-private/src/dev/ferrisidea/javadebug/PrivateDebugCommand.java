// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0
package dev.ferrisidea.javadebug;

import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.InetAddress;
import java.net.ServerSocket;
import java.net.Socket;
import java.net.StandardProtocolFamily;
import java.net.UnixDomainSocketAddress;
import java.nio.ByteBuffer;
import java.nio.channels.ServerSocketChannel;
import java.nio.channels.SocketChannel;
import java.nio.file.Path;
import java.util.List;

import org.eclipse.core.runtime.IProgressMonitor;
import org.eclipse.jdt.ls.core.internal.IDelegateCommandHandler;

import com.microsoft.java.debug.core.adapter.ProtocolServer;

/**
 * SPEC-0322 behaviour 4: serves the debug protocol on a Unix socket, or on the loopback address
 * where the caller names no path, instead of java-debug's listener on every interface.
 */
public final class PrivateDebugCommand implements IDelegateCommandHandler {
    private static final String START = "ferris-idea.java.startPrivateDebugSession";

    @Override
    public Object executeCommand(String command, List<Object> arguments, IProgressMonitor monitor)
            throws Exception {
        if (!START.equals(command)) {
            throw new UnsupportedOperationException(command);
        }
        if (arguments != null && !arguments.isEmpty() && arguments.get(0) instanceof String) {
            return serveUnix(Path.of((String) arguments.get(0)));
        }
        return serveLoopback();
    }

    private static String serveUnix(Path socket) throws IOException {
        ServerSocketChannel server = ServerSocketChannel.open(StandardProtocolFamily.UNIX);
        server.bind(UnixDomainSocketAddress.of(socket));
        daemon(() -> {
            try (server) {
                while (true) {
                    SocketChannel peer = server.accept();
                    daemon(() -> run(in(peer), out(peer), peer));
                }
            } catch (IOException ended) {
                // jdtls is shutting down.
            }
        });
        return socket.toString();
    }

    private static int serveLoopback() throws IOException {
        ServerSocket server = new ServerSocket(0, 1, InetAddress.getLoopbackAddress());
        int port = server.getLocalPort();
        daemon(() -> {
            try (server) {
                while (true) {
                    Socket peer = server.accept();
                    daemon(() -> {
                        try {
                            run(peer.getInputStream(), peer.getOutputStream(), peer);
                        } catch (IOException ended) {
                            // The client went away.
                        }
                    });
                }
            } catch (IOException ended) {
                // jdtls is shutting down.
            }
        });
        return port;
    }

    private static void run(InputStream input, OutputStream output, AutoCloseable peer) {
        try {
            new ProtocolServer(input, output, new PrivateContext()).run();
        } finally {
            try {
                peer.close();
            } catch (Exception ended) {
                // Nothing left to close.
            }
        }
    }

    private static void daemon(Runnable task) {
        Thread thread = new Thread(task, "Ferris IDEA debug listener");
        thread.setDaemon(true);
        thread.start();
    }

    // Plain reads and writes keep the two directions of the channel independent of each other.
    private static InputStream in(SocketChannel channel) {
        return new InputStream() {
            @Override
            public int read() throws IOException {
                byte[] one = new byte[1];
                return read(one, 0, 1) < 0 ? -1 : one[0] & 0xff;
            }

            @Override
            public int read(byte[] bytes, int offset, int length) throws IOException {
                return channel.read(ByteBuffer.wrap(bytes, offset, length));
            }
        };
    }

    private static OutputStream out(SocketChannel channel) {
        return new OutputStream() {
            @Override
            public void write(int b) throws IOException {
                write(new byte[] {(byte) b}, 0, 1);
            }

            @Override
            public void write(byte[] bytes, int offset, int length) throws IOException {
                ByteBuffer buffer = ByteBuffer.wrap(bytes, offset, length);
                while (buffer.hasRemaining()) {
                    channel.write(buffer);
                }
            }
        };
    }
}
