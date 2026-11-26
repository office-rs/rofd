import org.ofdrw.reader.OFDReader;
import org.ofdrw.sign.verify.OFDValidator;
import org.ofdrw.sign.verify.SignedDataValidateContainer;

import java.io.ByteArrayInputStream;
import java.io.ByteArrayOutputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
import java.util.zip.ZipEntry;
import java.util.zip.ZipInputStream;
import java.util.zip.ZipOutputStream;

/**
 * Validates OFD packages with ofdrw's own validator — the independent
 * cross-check of rofd-sign (the reference implementation of the archive
 * chain).
 *
 * <p>Each file is checked with every validate container ofdrw ships (SESV4,
 * GBT35275, SESV1, Digital), the way a verifier that tries them in turn
 * does. A file that mixes seals with digital signatures cannot be validated
 * in one pass (ofdrw's validator takes one container for the whole list and
 * refuses a signature of the other type), so such a file is validated once
 * per type, on a copy whose signature list holds only that type's records —
 * legitimate because the list is never protected.
 */
public class Verify {
    private static final Pattern RECORD = Pattern.compile("<(?:[A-Za-z0-9_]+:)?Signature\\b[^>]*>");
    private static final Pattern SIGN_TYPE = Pattern.compile("Type\\s*=\\s*\"Sign\"");

    public static void main(String[] args) throws Exception {
        int bad = 0;
        for (String path : args) {
            String verdict = validate(Files.readAllBytes(Path.of(path)));
            if (!verdict.startsWith("VALID")) {
                bad++;
            }
            System.out.println(verdict + "  " + path);
        }
        System.exit(bad == 0 ? 0 : 1);
    }

    static String validate(byte[] ofd) throws Exception {
        Map<String, byte[]> entries = readZip(ofd);
        String listName = entries.keySet().stream()
                .filter(n -> n.equals("Signatures.xml") || n.endsWith("/Signatures.xml"))
                .findFirst().orElse(null);
        if (listName == null) {
            return "UNSIGNED (declares no signature list)";
        }
        String xml = new String(entries.get(listName), StandardCharsets.UTF_8);
        List<int[]> spans = new ArrayList<>();
        int seals = 0;
        int signs = 0;
        Matcher m = RECORD.matcher(xml);
        while (m.find()) {
            spans.add(new int[] {m.start(), m.end()});
            if (SIGN_TYPE.matcher(m.group()).find()) {
                signs++;
            } else {
                seals++;
            }
        }
        if (seals > 0 && signs > 0) {
            String sealVerdict = validateWith(ofd, entries, listName, xml, spans, false, "seal");
            String signVerdict = validateWith(ofd, entries, listName, xml, spans, true, "digital");
            boolean ok = sealVerdict.startsWith("VALID") && signVerdict.startsWith("VALID");
            return (ok ? "VALID" : "INVALID") + " (mixed: " + seals + " seal(s) + " + signs
                    + " digital) seal part: " + sealVerdict + "; digital part: " + signVerdict;
        }
        return validateWith(ofd, entries, listName, xml, spans, signs > 0, "single");
    }

    /** Validate the file, or the copy holding only the records of `type`. */
    private static String validateWith(byte[] ofd, Map<String, byte[]> entries, String listName,
                                      String xml, List<int[]> spans, boolean sign,
                                      String what) throws Exception {
        byte[] bytes = ofd;
        if (!what.equals("single")) {
            StringBuilder kept = new StringBuilder();
            int at = 0;
            for (int[] span : spans) {
                kept.append(xml, at, span[0]);
                boolean isSign = SIGN_TYPE.matcher(xml.substring(span[0], span[1])).find();
                if (isSign == sign) {
                    kept.append(xml, span[0], span[1]);
                }
                at = span[1];
            }
            kept.append(xml, at, xml.length());
            entries.put(listName, kept.toString().getBytes(StandardCharsets.UTF_8));
            bytes = writeZip(entries);
        }
        return tryContainers(bytes, sign, what);
    }

    private static String tryContainers(byte[] ofd, boolean sign, String what) throws Exception {
        String[] names = sign
                ? new String[] {"GBT35275", "Digital", "SESV4", "SESV1"}
                : new String[] {"SESV4", "GBT35275", "SESV1", "Digital"};
        String firstError = null;
        for (String name : names) {
            Path file = Files.createTempFile("rofd-ofdverify-", ".ofd");
            Files.write(file, ofd);
            try (OFDReader reader = new OFDReader(file);
                 OFDValidator validator = new OFDValidator(reader)) {
                validator.setValidator(container(name));
                validator.exeValidate();
                return "VALID via " + name;
            } catch (Throwable e) {
                if (firstError == null) {
                    firstError = name + ": " + e;
                }
                if (System.getenv("TRACE") != null) {
                    e.printStackTrace(System.out);
                }
            } finally {
                Files.deleteIfExists(file);
            }
        }
        return "INVALID (no container accepted the " + what + " part; first error: " + firstError + ")";
    }

    static SignedDataValidateContainer container(String name) throws Exception {
        Class<?> c = Class.forName("org.ofdrw.sign.verify.container." + name + "ValidateContainer");
        return (SignedDataValidateContainer) c.getDeclaredConstructor().newInstance();
    }

    private static Map<String, byte[]> readZip(byte[] ofd) throws Exception {
        Map<String, byte[]> entries = new LinkedHashMap<>();
        try (ZipInputStream zip = new ZipInputStream(new ByteArrayInputStream(ofd))) {
            ZipEntry entry;
            while ((entry = zip.getNextEntry()) != null) {
                entries.put(entry.getName(), zip.readAllBytes());
            }
        }
        return entries;
    }

    private static byte[] writeZip(Map<String, byte[]> entries) throws Exception {
        ByteArrayOutputStream out = new ByteArrayOutputStream();
        try (ZipOutputStream zip = new ZipOutputStream(out)) {
            for (Map.Entry<String, byte[]> e : entries.entrySet()) {
                zip.putNextEntry(new ZipEntry(e.getKey()));
                zip.write(e.getValue());
                zip.closeEntry();
            }
        }
        return out.toByteArray();
    }
}
