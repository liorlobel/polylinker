
Universal Protein Resource (UniProt)
====================================


The Universal Protein Resource (UniProt), a collaboration between the European
Bioinformatics Institute (EBI), the SIB Swiss Institute of Bioinformatics, and
the Protein Information Resource (PIR), is comprised of three databases, each
optimized for different uses. The UniProt Knowledgebase (UniProtKB) is the
central access point for extensively curated protein information, including
function, classification and cross-references. The UniProt Reference Clusters
(UniRef) combine closely related sequences into a single record to speed up
sequence similarity searches. The UniProt Archive (UniParc) is a comprehensive
repository of all protein sequences, consisting only of unique identifiers and
sequences.

The UniProt Knowledgebase (UniProtKB) has been created from Swiss-Prot, TrEMBL
and PIR-PSD. It consists of two parts, one containing fully manually annotated
records and another one with computationally analysed records awaiting full
manual annotation. The two sections will be referred to as the Swiss-Prot
section of the UniProt Knowledgebase (UniProtKB/Swiss-Prot) and TrEMBL section
of the UniProt Knowledgebase (UniProtKB/TrEMBL), respectively. PIR-PSD release
80.0 of 31-Dec-2004 has been fully integrated into these sections. This was the
last release of PIR-PSD.

This directory, databases/uniprot/current_release/knowledgebase/complete,
contains the eight-weekly updates of the UniProt Knowledgebase, consisting of the 
Swiss-Prot Protein Knowledgebase (fully annotated curated entries) and the 
TrEMBL Protein Sequence Database (computer-generated entries enriched with 
automated classification and annotation). 
All files listed below contain the complete data sets corresponding to the
most recent eight-weekly release.

reldate.txt				Release number and dates
uniprot_sprot.dat.gz			Complete UniProtKB/Swiss-Prot data set 
                                        in flat file format
uniprot_trembl.dat.gz			Complete UniProtKB/TrEMBL data set 
                                        in flat file format
uniprot_sprot.xml.gz			Complete UniProtKB/Swiss-Prot data set 
                                        in XML format
uniprot_trembl.xml.gz			Complete UniProtKB/TrEMBL data set 
                                        in XML format
uniprot.xsd.gz				Schema definition for the UniProtKB XML 
					format
uniprot_sprot.fasta.gz			Complete UniProtKB/Swiss-Prot data set 
                                        in FASTA format
uniprot_trembl.fasta.gz			Complete UniProtKB/TrEMBL data set 
                                        in FASTA format
uniprot_sprot_varsplic.fasta.gz		Additional sequences of the UniProtKB/Swiss-Prot 
					data set that represent all annotated splice 
					variants
README


The /complete/docs subdirectory contains various UniProt documents.


--------------------------------------------------------------------------------
  LICENSE
--------------------------------------------------------------------------------
We have chosen to apply the Creative Commons Attribution 4.0 International
(CC BY 4.0) License (https://creativecommons.org/licenses/by/4.0/) to all
copyrightable parts of our databases.

(c) 2002-2024 UniProt Consortium

--------------------------------------------------------------------------------
  DISCLAIMER
--------------------------------------------------------------------------------
We make no warranties regarding the correctness of the data, and disclaim
liability for damages resulting from its use. We cannot provide unrestricted
permission regarding the use of the data, as some data may be covered by patents
or other rights.

Any medical or genetic information is provided for research, educational and
informational purposes only. It is not in any way intended to be used as a
substitute for professional medical advice, diagnosis, treatment or care.
