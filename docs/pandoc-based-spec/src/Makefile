TARGET=HotShot

all: paper

clean:
	rm hotshot.aux HotShot.bbl HotShot.blg HotShot.out HotShot.pdf HotShot.tex \
	HotShot.log  &> /dev/null || true

paper:
	@pandoc -s -F pandoc-crossref --natbib meta.yml --metadata-file=meta.yml --template=article.tex \
	--resource-path=.:src -N -f markdown+tex_math_single_backslash+citations -t latex+raw_tex \
	-o HotShot.tex HotShot.md
	@xelatex -interaction=nonstopmode HotShot.tex 
	@bibtex HotShot &> /dev/null
	@xelatex HotShot.tex &> /dev/null
	@xelatex HotShot.tex &> /dev/null

