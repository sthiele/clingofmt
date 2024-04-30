use super::*;

/// function to simplify tests
fn fmt_and_cmp(source_code: &str, res: &str) {
    let mut buf = Vec::new();
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_clingo::language())
        .expect("Error loading clingo grammar");

    let tree = parser.parse(&source_code, None).unwrap();
    let config = Config::default();

    format_program(&tree, source_code.as_bytes(), &mut buf, false, &config).unwrap();
    let parse_res = std::str::from_utf8(&buf).unwrap();
    assert_eq!(parse_res, res)
}

#[test]
fn test_pass_new() {
    fmt_and_cmp(" \n \n ", "");
    fmt_and_cmp("% bla blub       ", "% bla blub\n");
    fmt_and_cmp("% bla\n% blub       ", "% bla\n% blub\n");
    fmt_and_cmp(
        "%* multi  \n    line\n    comment  *%",
        "%* multi  \n    line\n    comment  *%\n",
    );
    fmt_and_cmp(" pred(something).        ", "pred(something).\n");
    fmt_and_cmp(
        " pred(something).     % bla   ",
        "pred(something).\n\n% bla\n",
    );
    fmt_and_cmp("% bla blub\n   a:-b.   ", "% bla blub\na :-\n    b.\n");
    fmt_and_cmp(
        "% fact block\n a(1).\n a(2). a(3).",
        "% fact block\na(1). a(2). a(3).\n",
    );
    fmt_and_cmp(
        "%* fact block *%  \n  a(1).   \na(2). a(3).",
        "%* fact block *%\na(1). a(2). a(3).\n",
    );
    fmt_and_cmp(
        "%* fact block *%  \n  a(1%*bla*%   ).   \na(2). a(3).",
        "%* fact block *%\na(1%*bla*%). a(2). a(3).\n",
    );
    fmt_and_cmp(
        "% fact block1 \n  a(1%*bla*%   ).  \na(2). a(3).%* fact block2 *%  b(1%*bla*%   ).  \nb(2). b(3).",
        "% fact block1\na(1%*bla*%). a(2). a(3).\n\n%* fact block2 *%\nb(1%*bla*%). b(2). b(3).\n",
    );
}

#[test]
fn test_include_section() {
    let source = r#"% Comment for include block
  #include "file1.lp".#include "file2.lp".     #include "file3.lp".
"#;
    let result = r#"% Comment for include block
#include "file1.lp".
#include "file2.lp".
#include "file3.lp".
"#;
    fmt_and_cmp(source, result);

    let source = r#"% Comment for include block
#include "file1.lp".#include "file2.lp".     #include "file3.lp". some(fact).
"#;
    let result = r#"% Comment for include block
#include "file1.lp".
#include "file2.lp".
#include "file3.lp".

some(fact).
"#;
    fmt_and_cmp(source, result);

    let source = r#"% Comment for include block
#include "file1.lp".#include "file2.lp".     #include "file3.lp". #show pred/3.
"#;
    let result = r#"% Comment for include block
#include "file1.lp".
#include "file2.lp".
#include "file3.lp".

#show pred/3.
"#;
    fmt_and_cmp(source, result);

    let source = r#"% Comment for include block
#include "file1.lp".#include "file2.lp".     #include "file3.lp". a :- rule.
"#;
    let result = r#"% Comment for include block
#include "file1.lp".
#include "file2.lp".
#include "file3.lp".

a :-
    rule.
"#;
    fmt_and_cmp(source, result);

    let source = r#"% Comment for include block
#include "file1.lp".#include "file2.lp".     #include "file3.lp". %comment
a:-rule.
"#;
    let result = r#"% Comment for include block
#include "file1.lp".
#include "file2.lp".
#include "file3.lp".

%comment
a :-
    rule.
"#;
    fmt_and_cmp(source, result);
}
#[test]
fn test_pass_old() {
    let source = r#"% Derive (varying) atoms
atom(A):-model(M),true(M,A).vary(A):-model(M),atom(A),not true(M,A).
% Derive lower bound LB and upper bound UB for size of prime implicants
% - LB: minimum number of varying atoms s.t. interpretations don't exceed models
% - UB: minimum of number of varying atoms and number of non-models
varies(X):-X = #count{ A : vary(A) }.models(Y):-Y = #count{ M : model(M) }.:- models(0). % must have some model
minsize(Y,2**X,0) :-varies(X),models(Y),1 < Y. % nothing varies if one model
minsize(Y,Z/2,L+1):-minsize(Y,Z,L),Y < Z.
bounds(L,(X+F-|X-F|)/2):-varies(X),minsize(Y,Z,L),not minsize(Y,Z/2,L+1),
                          F = 2**X-Y.
% Select literals for prime implicant
  select(A,1)         :-atom(A),not vary(A).{ select(A,0..1) } < 2:-vary(A),not bounds(0,0).
selected(A):-select(A,V),vary(A).
% Check lower and upper bounds via "Sinz counter" on selected varying atoms
index(A,I):-vary(A),I = #count{ B : vary(B),B <= A },not bounds(0,0).
counter(I,1)  :-index(A,I),bounds(L,U),L <= I,selected(A).counter(I,C+1):-index(A,I),bounds(L,U),C < U,selected(A),counter(I+1,C).counter(I,C)  :-index(A,I),bounds(L,U),L < C+I,counter(I+1,C).
:- bounds(L,U),0 < L,not counter(1,L).:- bounds(L,U),index(A,I),selected(A),counter(I+1,U).
% Derive models excluded by (some) selected literal
exclude(M,A):-model(M),select(A,0),true(M,A).exclude(M,A):-model(M),select(A,1),not true(M,A).
excluded(M):-exclude(M,A).
% Check that all interpretations extending prime implicant are models
:- bounds(L,U),varies(X),models(Y),
   #sum{ 2**(X-Z) : Z = L+1..X,not counter(1,Z);
              1,M : excluded(M) } >= Y.
% Check that removing any literal of prime implicant yields some non-model
:- bounds(L,U),varies(X),models(Y),index(A,I),   #sum{ 2**(X-Z) : Z = L..X,not counter(1,Z+1);   1,M : exclude(M,B),B != A } < Y.
% Display literals of prime implicant
#show select/2.
#show a(A) : b(A), field(AN).
#show select("root",X).
              n(s).bb(x).
output(@fmt(("The @fmt() function is flexible enough to take multi-line ",
             "strings containing many placeholders: {} and ",
             "{} and {} outputs"), (X,Y,Z))) :- num(X),string(Y),constant(Z).
sel_vat(H, V) :- sel_vat(N,W) : cons(Identifier,var(N,W));
   subgraph(N) : cons(Identifier, has_x("strong",N))%*jjj*%;
   %c1
   has_con(F, T, Na, Index) 
   %c0
   %c01
   : cons(Identifier, has_con("strong", F, T, Na, Index));
   %c2
   %c3
   not has_con(F, _, Na, Index) : cons(Identifier, has_con("weak", F, Na, Index));  %* c2
    sss *%
   cons(Identifier,tail(H,V)).

    bla%aa 
    %bb  
    :-%aa 
    %bb
     varies(X),
    #sum %aa 
    %bb
    { %aa 
    %bb
        2**(X-Z)%aa 
    %bb 
    :%aa 
    %bb
            Z = L+1..X,%aa 
    %bb
            not counter(1, Z); %aa 
    %bb
        1, M :
            excluded(M)
    } %aa 
    %bb
     >= %aa 
    %bb
     Y %aa 
    %bb
    ,models(Y).

:- bla(1,2).
#external a.
#const c= "Dd".
#minimize{fff}.
#maximise {X:ccc(X)}.
#include "fail1.lp".
#heuristic heu.[ha,ak]
#defined bla/2.
#project p("s",X).

#theory test {
    &q/1 : t, body;
    &r/0 : t, { < }, t, directive
}.
#edge(a,b).#edge(c,d).

"#;
    let result = r#"% Derive (varying) atoms
atom(A) :-
    model(M), true(M, A).

vary(A) :-
    model(M), atom(A), not true(M, A).

% Derive lower bound LB and upper bound UB for size of prime implicants
% - LB: minimum number of varying atoms s.t. interpretations don't exceed models
% - UB: minimum of number of varying atoms and number of non-models
varies(X) :-
    X = #count {
        A : vary(A)
    }.

models(Y) :-
    Y = #count {
        M : model(M)
    }.

 :- models(0).

% must have some model
minsize(Y, 2**X, 0) :-
    varies(X), models(Y), 1 < Y.

% nothing varies if one model
minsize(Y, Z/2, L+1) :-
    minsize(Y, Z, L), Y < Z.

bounds(L, (X+F- | X-F | )/2) :-
    varies(X), minsize(Y, Z, L), not minsize(Y, Z/2, L+1), F = 2**X-Y.

% Select literals for prime implicant
select(A, 1) :-
    atom(A), not vary(A).

{
    select(A, 0..1)
} < 2 :-
    vary(A), not bounds(0, 0).

selected(A) :-
    select(A, V), vary(A).

% Check lower and upper bounds via "Sinz counter" on selected varying atoms
index(A, I) :-
    vary(A), I = #count {
        B : vary(B), B <= A
    }, not bounds(0, 0).

counter(I, 1) :-
    index(A, I), bounds(L, U), L <= I, selected(A).

counter(I, C+1) :-
    index(A, I), bounds(L, U), C < U, selected(A), counter(I+1, 
        C).

counter(I, C) :-
    index(A, I), bounds(L, U), L < C+I, counter(I+1, C).

 :- bounds(L, U), 0 < L, not counter(1, L).

 :- bounds(L, U), index(A, I), selected(A), counter(I+1, U).

% Derive models excluded by (some) selected literal
exclude(M, A) :-
    model(M), select(A, 0), true(M, A).

exclude(M, A) :-
    model(M), select(A, 1), not true(M, A).

excluded(M) :-
    exclude(M, A).

% Check that all interpretations extending prime implicant are models
 :- bounds(L, U), varies(X), models(Y), #sum {
        2**(X-Z) : Z = L+1..X, not counter(1, Z);
        1, M : excluded(M)
    } >= Y.

% Check that removing any literal of prime implicant yields some non-model
 :- bounds(L, U), varies(X), models(Y), index(A, I), #sum {
        2**(X-Z) : Z = L..X, not counter(1, Z+1);
        1, M : exclude(M, B), B != A
    } < Y.

% Display literals of prime implicant
#show select/2.
#show a(A) : b(A), field(AN).
#show select("root", X).

n(s). bb(x).

output(@fmt(("The @fmt() function is flexible enough to take multi-line ", 
            "strings containing many placeholders: {} and ", 
            "{} and {} outputs"), (X, Y, Z))) :-
    num(X), string(Y), constant(Z).

sel_vat(H, V) :-
    sel_vat(N, W) : cons(Identifier, var(N, W));
    subgraph(N) : cons(Identifier, has_x("strong", N))%*jjj*%;
    %c1
    has_con(F, T, Na, Index)%c0
    %c01
     : cons(Identifier, has_con("strong", F, T, Na, Index));
    %c2
    %c3
    not has_con(F, _, Na, Index) : cons(Identifier, has_con("weak", 
                F, Na, Index));
    %* c2
    sss *%cons(Identifier, tail(H, V)).

bla%aa
%bb
 :-
    %aa
    %bb
    varies(X), #sum %aa
    %bb
    {
        %aa
        %bb
        2**(X-Z)%aa
        %bb
         : %aa
            %bb
            Z = L+1..X, %aa
            %bb
            not counter(1, Z);
        %aa
        %bb
        1, M : excluded(M)
    }%aa
    %bb
     >= %aa
    %bb
    Y%aa
    %bb
    , models(Y).

 :- bla(1, 2).

#external a.

#const c="Dd".

#minimize {
    fff
}.

#maximise {
    X : ccc(X)
}.

#include "fail1.lp".

#heuristic heu. [ha, ak]

#defined bla/2.

#project p("s", X).

#theory test {
    &q/1 : t, body;
    &r/0 : t, { < }, t, directive
}.

#edge(a, b). #edge(c, d).
"#;
    fmt_and_cmp(source, result);
}
