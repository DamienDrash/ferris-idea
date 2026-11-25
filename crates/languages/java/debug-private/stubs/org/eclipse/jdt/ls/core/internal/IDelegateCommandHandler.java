// SPDX-License-Identifier: Apache-2.0
// Compile-time declaration only, never packaged: the name and signature jdtls binds at run time.
package org.eclipse.jdt.ls.core.internal;

import java.util.List;
import org.eclipse.core.runtime.IProgressMonitor;

public interface IDelegateCommandHandler {
    Object executeCommand(String commandId, List<Object> arguments, IProgressMonitor monitor) throws Exception;
}
