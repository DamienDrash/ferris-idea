// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0
package dev.idecraft.javadebug;

import java.io.File;
import java.io.IOException;
import java.net.InetAddress;
import java.net.ServerSocket;
import java.net.Socket;
import java.net.SocketTimeoutException;
import java.nio.charset.StandardCharsets;
import java.util.List;
import java.util.Map;

import org.eclipse.jdi.internal.VirtualMachineImpl;
import org.eclipse.jdi.internal.VirtualMachineManagerImpl;

import com.microsoft.java.debug.core.DebugUtility;
import com.microsoft.java.debug.plugin.internal.AdvancedLaunchingConnector;
import com.sun.jdi.VirtualMachine;
import com.sun.jdi.connect.Connector.Argument;
import com.sun.jdi.connect.IllegalConnectorArgumentsException;
import com.sun.jdi.connect.VMStartException;

/**
 * Starts the program under JDWP and waits for it on a loopback port, closed again once the program
 * has connected; the platform's own connector waits on every interface.
 */
final class LoopbackLaunchingConnector extends AdvancedLaunchingConnector {
    private static final int START_MILLIS = 10_000;
    private static final int POLL_MILLIS = 200;

    LoopbackLaunchingConnector(VirtualMachineManagerImpl manager) {
        super(manager);
    }

    @Override
    public VirtualMachine launch(Map<String, ? extends Argument> arguments)
            throws IOException, IllegalConnectorArgumentsException, VMStartException {
        try (ServerSocket waiting = new ServerSocket(0, 1, InetAddress.getLoopbackAddress())) {
            waiting.setSoTimeout(POLL_MILLIS);
            String address = waiting.getInetAddress().getHostAddress() + ":" + waiting.getLocalPort();
            Process program = start(arguments, address);
            Socket peer = await(waiting, program);
            VirtualMachine machine = establishedConnection(new JdwpConnection(peer));
            ((VirtualMachineImpl) machine).setLaunchedProcess(program);
            return machine;
        }
    }

    private static Process start(Map<String, ? extends Argument> arguments, String address) throws IOException {
        String separator = File.separator;
        String command = "\"" + arguments.get(DebugUtility.HOME).value() + separator + "bin" + separator
                + arguments.get(DebugUtility.EXEC).value() + "\""
                + " -Xrunjdwp:transport=dt_socket,address=" + address + ",server=n,suspend="
                + (Boolean.parseBoolean(arguments.get(DebugUtility.SUSPEND).value()) ? "y" : "n");
        String options = arguments.get(DebugUtility.OPTIONS).value();
        command += (options == null ? "" : " " + options) + " " + arguments.get(DebugUtility.MAIN).value();
        List<String> words = DebugUtility.parseArguments(command);
        String folder = arguments.get(DebugUtility.CWD).value();
        File directory = folder != null && new File(folder).isDirectory() ? new File(folder) : null;
        String[] environment = environment(arguments.get(DebugUtility.ENV).value());
        return Runtime.getRuntime().exec(words.toArray(new String[0]), environment, directory);
    }

    private static String[] environment(String encoded) {
        try {
            return DebugUtility.decodeArrayArgument(encoded);
        } catch (IllegalArgumentException notEncoded) {
            return null;
        }
    }

    private static Socket await(ServerSocket waiting, Process program) throws IOException, VMStartException {
        long deadline = System.currentTimeMillis() + START_MILLIS;
        while (System.currentTimeMillis() < deadline) {
            if (!program.isAlive()) {
                String output = new String(program.getErrorStream().readAllBytes(), StandardCharsets.UTF_8);
                throw new VMStartException("VM exited with status " + program.exitValue() + ": " + output, program);
            }
            try {
                Socket peer = waiting.accept();
                if (JdwpConnection.handshake(peer)) {
                    return peer;
                }
                peer.close();
            } catch (SocketTimeoutException keepWaiting) {
                // Check the program again.
            }
        }
        program.destroy();
        throw new VMStartException("VM did not connect within " + START_MILLIS + " ms", program);
    }
}
