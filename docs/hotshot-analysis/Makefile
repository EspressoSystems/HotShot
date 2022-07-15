analysis.pdf: analysis.tex
	rm -r build || true
	mkdir build
	latexmk -pdf -outdir=build/ analysis.tex
	cp build/*.pdf .

