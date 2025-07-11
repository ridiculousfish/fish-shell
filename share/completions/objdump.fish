complete -c objdump -l archive-headers -s a -d "Display archive header information"
complete -c objdump -l file-headers -s f -d "Display contents of the overall file header"
complete -c objdump -l private-headers -s p -d "Display object format specific file header contents"
complete -c objdump -l private -s P -d "Display object format specific contents" -x
complete -c objdump -l header -s h -d "Display contents of section headers"
complete -c objdump -l section-header -s h -d "Display content of section headers"
complete -c objdump -l all-headers -s x -d "Display the contents of all headers"
complete -c objdump -l disassemble -s d -d "Display assembler contents of executable sections"
complete -c objdump -l disassemble-all -s D -d "Display assembler contents of all sections"
complete -c objdump -l source -s S -d "Intermix source code with disassembly"
complete -c objdump -l full-contents -s s -d "Display full contents of all sections requested"
complete -c objdump -l debugging -s g -d "Display debug information in object file"
complete -c objdump -l debugging-tags -s e -d "Display debug information using ctags style"
complete -c objdump -l stabs -s G -d "Display (in raw form) any STABS info in file"
complete -c objdump -l dwarf -x -d "Display DWARF info in file" -a "rawline decodedline info abbrev pubnames aranges macro frames frames-interp str loc Ranges pubtypes gdb_index trace_info trace_abbrev trace_aranges addr cu_index"
complete -c objdump -l syms -s t -d "Display contents of symbol table(s)"
complete -c objdump -l dynamic-syms -s T -d "Display contents of dynamic symbol table"
complete -c objdump -l reloc -s r -d "Display relocation entries in file"
complete -c objdump -l dynamic-reloc -s R -d "Display dynamic relocation entries in file"
complete -c objdump -l version -s v -d "Display version number"
complete -c objdump -l info -s i -d "List object formats and architectures supported"
complete -c objdump -l help -s H -d "Display help"
complete -c objdump -l target -s b -d "Specify target object format" -x -a "elf64-x86-64 elf32-i386 elf32-iamcu elf32-x86-64 a.out-i386-linux pei-i386 pei-x86-64 elf64-l1om elf64-k1om elf64-little elf64-big elf32-little elf32-big plugin srec symbolsrec verilog tekhex binary ihex"
complete -c objdump -l architecture -s m -d "Specify target architecture" -x -a "i386 i386:x86-64 i386:x64-32 i8086 i386:intel i386:x86-64:intel i386:x64-32:intel i386:nacl i386:x86-64:nacl i386:x64-32:nacl iamcu iamcu:intel l1om l1om:intel k1om k1om:intel plugin"
complete -c objdump -l section -s j -d "Only display information for given section" -x
complete -c objdump -l disassembler-options -s M -d "Pass given options on to disassembler" -x
complete -c objdump -l disassembler-color -d "Control disassembler syntax highlighting style" -x -a "off terminal on extended"
complete -c objdump -l endian -x -d "Set format endianness when disassembling" -a "big little"
complete -c objdump -o EB -d "Assume big endian format when disassembling"
complete -c objdump -o EL -d "Assume little endian format when disassembling"
complete -c objdump -l file-start-context -d "Include context from start of file (with -S)"
complete -c objdump -l include -s I -f -d "Add given directory to search list from source files" -x
complete -c objdump -l line-numbers -s l -d "Include line numbers and filenames in output"
complete -c objdump -l file-offsets -s F -d "Include file offsets when displaying information"
complete -c objdump -l demangle -s C -d "Decode mangled/processed symbol names" -x -a "auto gnu lucid arm hp edg gnu-v3 java gnat"
complete -c objdump -l wide -s w -d "Format output for more than 80 columns"
complete -c objdump -l disassemble-zeroes -s z -d "Do not skip blocks of zeroes when disassembling"
complete -c objdump -l start-address -d "Only process data whose address is >= given address" -x
complete -c objdump -l stop-address -d "Only process data whose address is <= given address" -x
complete -c objdump -l prefix-addresses -d "Print complete address alongside disassembly"
complete -c objdump -l show-raw-insn -d "Display hex alongside symbolic disassembly"
complete -c objdump -l no-show-raw-insn -d "Don't display hex alongside symbolic disassembly"
complete -c objdump -l insn-width -x -d "Display specified number of bytes on single line for -d"
complete -c objdump -l adjust-vma -x -d "Add offset to all displayed section address"
complete -c objdump -l special-syms -d "Include special symbols in symbol dumps"
complete -c objdump -l prefix -x -d "Add given prefix to absolute paths for -S"
complete -c objdump -l prefix-strip -x -d "Strip initial directory names for -S"
complete -c objdump -l dwarf-depth -x -d "Do not display DIEs at given depth or greater"
complete -c objdump -l dwarf-start -x -d "Display DIEs starting with given number"
complete -c objdump -l dwarf-check -d "Make additional dwarf internal consistency checks"
