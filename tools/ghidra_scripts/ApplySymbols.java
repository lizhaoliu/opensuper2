// Rename functions from a TSV (addr<TAB>name<TAB>comment), then decompile all
// functions to the output file. Args: <symbols.tsv> <out.c>
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.*;
import ghidra.program.model.listing.*;
import ghidra.program.model.symbol.SourceType;
import ghidra.program.model.address.Address;
import java.io.*;
import java.nio.file.*;

public class ApplySymbols extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        int renamed = 0, skipped = 0;
        for (String line : Files.readAllLines(Paths.get(args[0]))) {
            String[] f = line.split("\t");
            if (f.length < 2 || f[0].equals("addr")) continue;
            String name = f[1].trim().replaceAll("[^A-Za-z0-9_]", "_");
            if (name.isEmpty()) continue;
            try {
                Address a = toAddr(Long.parseLong(f[0].trim().replace("0x", ""), 16));
                Function fn = getFunctionAt(a);
                if (fn == null) { skipped++; continue; }
                fn.setName(name, SourceType.USER_DEFINED);
                if (f.length > 2 && !f[2].trim().isEmpty()) fn.setComment(f[2].trim());
                renamed++;
            } catch (Exception e) { skipped++; }
        }
        println("renamed " + renamed + ", skipped " + skipped);
        DecompInterface ifc = new DecompInterface();
        ifc.openProgram(currentProgram);
        try (PrintWriter w = new PrintWriter(new FileWriter(args[1]))) {
            for (Function fn : currentProgram.getFunctionManager().getFunctions(true)) {
                DecompileResults r = ifc.decompileFunction(fn, 60, monitor);
                w.println("// ==== " + fn.getName() + " @ " + fn.getEntryPoint());
                if (fn.getComment() != null) w.println("// " + fn.getComment().replace("\n", " "));
                w.println(r.decompileCompleted() ? r.getDecompiledFunction().getC() : "// decompile failed: " + r.getErrorMessage());
            }
        }
    }
}
