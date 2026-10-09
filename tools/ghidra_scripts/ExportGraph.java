// Export per-function info: entry, size, callees, string refs, data refs (JSON lines).
import ghidra.app.script.GhidraScript;
import ghidra.program.model.listing.*;
import ghidra.program.model.symbol.*;
import ghidra.program.model.address.*;
import ghidra.program.model.data.*;
import java.io.*;
import java.util.*;

public class ExportGraph extends GhidraScript {
    String esc(String s) {
        StringBuilder b = new StringBuilder();
        for (char c : s.toCharArray()) {
            if (c == '"' || c == '\\') b.append('\\').append(c);
            else if (c < 0x20 || c > 0x7e) b.append(String.format("\\u%04x", (int) c));
            else b.append(c);
        }
        return b.toString();
    }
    @Override
    public void run() throws Exception {
        String out = getScriptArgs()[0];
        Listing listing = currentProgram.getListing();
        ReferenceManager rm = currentProgram.getReferenceManager();
        try (PrintWriter w = new PrintWriter(new FileWriter(out))) {
            for (Function f : currentProgram.getFunctionManager().getFunctions(true)) {
                Set<String> callees = new TreeSet<>();
                for (Function c : f.getCalledFunctions(monitor)) callees.add(c.getEntryPoint().toString());
                Set<String> strs = new TreeSet<>();
                Set<String> globals = new TreeSet<>();
                for (Instruction ins : listing.getInstructions(f.getBody(), true)) {
                    for (Reference r : ins.getReferencesFrom()) {
                        Address to = r.getToAddress();
                        if (!to.isMemoryAddress()) continue;
                        Data d = listing.getDataAt(to);
                        if (d != null && d.hasStringValue()) strs.add(esc(String.valueOf(d.getValue())));
                        else if (r.getReferenceType().isData() && listing.getFunctionAt(to) == null) globals.add(to.toString());
                    }
                }
                int ncallers = f.getCallingFunctions(monitor).size();
                w.print("{\"addr\":\"" + f.getEntryPoint() + "\",\"name\":\"" + esc(f.getName()) + "\",\"size\":" + f.getBody().getNumAddresses()
                        + ",\"ncallers\":" + ncallers + ",\"callees\":[");
                int i = 0; for (String c : callees) w.print((i++ > 0 ? "," : "") + "\"" + c + "\"");
                w.print("],\"strings\":[");
                i = 0; for (String s : strs) w.print((i++ > 0 ? "," : "") + "\"" + s + "\"");
                w.print("],\"globals\":[");
                i = 0; for (String s : globals) w.print((i++ > 0 ? "," : "") + "\"" + s + "\"");
                w.println("]}");
            }
        }
    }
}
