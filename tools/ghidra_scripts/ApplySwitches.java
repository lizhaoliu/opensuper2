// Apply recovered jump tables (file: "func jmp t1,t2,..." lines) as switch overrides,
// then decompile the affected functions into the output file.
// Args: <switches.txt> <out.c>
import ghidra.app.script.GhidraScript;
import ghidra.app.cmd.function.CreateFunctionCmd;
import ghidra.app.decompiler.*;
import ghidra.program.model.address.*;
import ghidra.program.model.listing.*;
import ghidra.program.model.pcode.JumpTable;
import ghidra.program.model.symbol.*;
import java.io.*;
import java.util.*;

public class ApplySwitches extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        List<Function> funcs = new ArrayList<>();
        try (BufferedReader r = new BufferedReader(new FileReader(args[0]))) {
            String line;
            while ((line = r.readLine()) != null) {
                if (line.startsWith("#") || line.isBlank()) continue;
                String[] p = line.trim().split("\\s+");
                Address jmp = toAddr(Long.parseLong(p[1], 16));
                ArrayList<Address> dests = new ArrayList<>();
                for (String t : p[2].split(",")) dests.add(toAddr(Long.parseLong(t, 16)));
                for (Address d : dests) {
                    if (getInstructionAt(d) == null) disassemble(d);
                }
                Instruction ins = getInstructionAt(jmp);
                Function f = getFunctionContaining(jmp);
                if (ins == null || f == null) { println("skip " + p[1]); continue; }
                for (Address d : dests) ins.addOperandReference(0, d, RefType.COMPUTED_JUMP, SourceType.USER_DEFINED);
                new JumpTable(jmp, dests, true, 0).writeOverride(f);
                CreateFunctionCmd.fixupFunctionBody(currentProgram, f, monitor);
                if (!funcs.contains(f)) funcs.add(f);
            }
        }
        DecompInterface ifc = new DecompInterface();
        ifc.openProgram(currentProgram);
        try (PrintWriter w = new PrintWriter(new FileWriter(args[1]))) {
            for (Function f : funcs) {
                DecompileResults res = ifc.decompileFunction(f, 120, monitor);
                w.println("// ==== " + f.getName() + " @ " + f.getEntryPoint());
                w.println(res.decompileCompleted() ? res.getDecompiledFunction().getC() : "// failed: " + res.getErrorMessage());
            }
        }
        println("applied " + funcs.size() + " functions");
    }
}
